# AS7772 G4: smoke re-run, load through SONiC on the Rust SAI, soak monitor (agent H3, 2026-11-23 UTC)

System **as7772-rust from committed `50a6aa7`** (`DEPLOY_REV=50a6aa7 DEPLOY_ATTR=as7772-rust deploy.sh boot`, then a reboot):
`64296gcvk4yyhq2k8yjxwib67kil0bxz` (before: `zvih1crw...`), libsai / tools from `iicq634b...-sai-openbcm-rs`, sai.profile
`qm59rnyp...`. The working tree was not built. Box clock is PST; times here are UTC. No PHY hang, no power cycle.
Lab CAK grepped in every file here and in `loadtest/sonic-ring.sh`: 0 hits.

| file | UTC | what | result | sha256 (16) |
| --- | --- | --- | --- | --- |
| a0-deploy-boot.txt | 12:42Z | deploy.sh boot (DEPLOY_REV=50a6aa7, as7772-rust) | rc 0, 64296gcv default | 86c30d6986d516e8 |
| a1-reboot-health-mdio-summary.txt | 12:45Z | reboot: system, failed units, MDIO tool status | 0 failed, 32/32, summary 101.0 s = status.json seconds 101.0 | 26c4f3835d1debd3 |
| a2-macsec-up-check.txt | 12:45Z | macsec-up-check.sh after the reboot | PASS 14.7 s | afc8f82dc1142332 |
| b1-smoke-tables-and-recreate.txt | 12:46Z | sai-openbcm-smoke, SONiC stopped, board profile + PHY tables; then --recreate | 22 PASS 1 SKIP 0 FAIL; --recreate 23 PASS 0 FAIL | 83348e72816148ee |
| c0-configdb-before-and-checks.txt | 12:47Z | CONFIG_DB keys, INTERFACE, CoPP, sysctls, LLDP, oper-up before the load | 4 up, 4 LLDP = self | 89499d7bb53c2a54 |
| c0-phystat-before.jsonl | 12:50Z | phystat.py baseline (clears FEC counters) | 4/4 read; boot-time FEC counts | 5861e52dbfed7c88 |
| c0-sample-idle.txt | 12:50Z | sampler, idle | reference | 7d9b2a47657603f3 |
| c1-ring-up.txt | 12:52Z | sonic-ring.sh up: VLANs, members, static FDB through swssconfig, read back | 4 Static entries, MKA 8 SAs | 16dfccba53011d59 |
| c2-journal-err-phystat-collision.txt | 12:50Z | journal errors during the baseline phystat | syncd MACsec PN sequence failures on 4 PHYs | 66d285e815c9f47a |
| d0-ring-samples.txt | 12:53-13:22Z | sampler every ~56 s (SAI + ASIC + CPU + netdev counters, temps, syncd) | 32 samples, 0 guard trips | 723d7dd3d7e43cef |
| d1-ring-macsec-samples.txt | 12:53-13:21Z | show macsec every ~5 min (keys filtered) | 0 ingress errors | f1d5008ee10a5b4e |
| d2-ring-macsec-poststop.txt | 13:23Z | show macsec after the stop | encrypted = accepted per SC | 5861057f260e9bcf |
| d3-ring-asic-full-counters.txt | 12:53/13:23Z | all non-zero ASIC counters before inject, before and after stop | no error counter | 40212822a0a0d440 |
| d4-ring-stop-and-journal.txt | 13:23Z | pre/stop/post samples, l2, journal err/warnings since 12:51Z | 0 loss; 4 orchagent removeBridgePort warnings | c9cc2210848ca72d |
| d5-phystat-after.txt | 13:24-13:25Z | phystat after the stop (two attempts) + journal | 0x01 FEC 0/0; 0x17f 0/12 (suspect); 0x00, 0x17e FW Message failure | 571a14ab93bd31ea |
| d6-ring-down.txt | 13:25Z | sonic-ring.sh down | CONFIG_DB restored | 3598b3934c5f0f2d |
| d7-ring-conservation.json | 13:25Z | computed cable / switch balances, per-interval rates | 0 frame / byte difference | 488b49dd07924412 |
| e0-final-reboot-state.txt | 13:29Z | after reboot: system, failed units, MDIO summary, CONFIG_DB keys | 64296gcv booted, 0 failed, 101.2 = 101.2 s, keys as c0 | 19d4cb462a64497a |
| e1-final-macsec-up-check.txt | 13:29Z | macsec-up-check.sh | PASS 14.9 s | 7cbcd19617656bc7 |
| e2-final-macsec-traffic-test.txt | 13:30Z | macsec-traffic-test.sh | 6400 pings 0 lost, encrypted = accepted, 0 errors | 48c384d3c33a7afd |
| e3-soak-monitor-start.txt | 13:31Z | soak monitor install + first run | timer every 5 min | 042b015092b0f997 |
| e3-soak-monitor-soak.sh.txt | 13:31Z | copy of /root/soak-rust/soak.sh |  | 6d3b51b56d5945c0 |

## Load design ([../../loadtest/sonic-ring.sh](../../loadtest/sonic-ring.sh))
- Programmed only through SONiC: CONFIG_DB `VLAN` / `VLAN_MEMBER` (Vlan100 = Ethernet0 + Ethernet120, Vlan200 = Ethernet4 +
  Ethernet124, untagged) and four static FDB entries in APPL_DB `FDB_TABLE` loaded with swss's own `swssconfig` =>
  orchagent => syncd => Rust libsai => ASIC (`l2 show` read back: 4 `Static` entries). The diag shell was used for reads only.
- Topology = `ring.sh`'s two counter-rotating rings over both DACs; the CPU injected 740 frames per ring (ring.sh's mix,
  64-9000 B, ethertype 0x88b5) through the port netdevs (KNET, directed to the port). **Known unicast, not a flood ring**:
  this SAI makes the CPU a tagged member of every VLAN (`sdk_glue.c` `ob_sdk_vlan_create`: "floods reach KNET"), so an
  unknown-unicast / broadcast ring would copy every frame to the CPU at every hop. Guards: IPv6 off on the four netdevs,
  Bridge, Vlan100/200 (neigh_discovery is a `copy` CoPP trap: a stray ND multicast would circulate and be copied); the
  kernel bridge ports set to no learning / no flooding (the kernel has no bridge netfilter, `CONFIG_NF_TABLES_BRIDGE` unset),
  so a CPU copy cannot be bridged back out (Bridge tx stayed at 20 frames). MACsec left as SONiC runs it (MKA, XPN-256,
  switch MAC IPG 48 set by the SAI for MACsec ports).
- To make the far ends bridge ports, their `INTERFACE|Ethernet4/124` entries (mac_addr only) were deleted; the netdevs kept
  their addresses, MKA was not restarted (8 SAs open throughout). Stop = the four `VLAN_MEMBER` entries deleted.
- House network: only the four DAC ports oper up, all four LLDP neighbours this switch (`check`, c0), before starting.

## Results
- **Rate:** 30 min 15 s (12:53:04-13:23:19Z), every port **92.07 Gb/s of frames, 17.69 Mpps per direction** (totals /
  duration; 31 sampler intervals 90.3-93.9 Gb/s, the spread is the 1-s timestamps of the sampler); ring.sh with SONiC
  stopped and MACsec (IPG 48): 92.05 Gb/s. 32.10 x 10^9 frames per port per direction.
- **Cable loss 0:** ASIC TPKT/TBYT delta of each port = RPKT/RBYT delta of its peer exactly, all four directions
  (ce0->ce1 32098519875, ce1->ce0 32098040658, ce30->ce31 32098039917, ce31->ce30 32098519875 frames; bytes equal too). Before the ring the counters already differed by
  1 / 9 / 9 / 1 frames (frames sent at link-up before the peer's MACsec was up); the deltas compare equal.
- **Switch loss 0:** injected 1480 = drained 1480: 740 ended at ce0 egress (`TDBGC3.ce0` 740 = SAI `IF_OUT_DISCARDS` 740 on
  Ethernet0), 740 reached the CPU once at ce0 (Ethernet0 netdev rx +740 over the 1453 control frames the other three
  netdevs received); netdev tx (control + injected) = netdev rx + 740 exactly. No other drop / error counter moved
  (RFCS, RERPKT, RJBR, RFRG, RUND, TERR 0; SAI in/out errors 0).
- **MACsec:** per SC OUT_PKTS_ENCRYPTED = peer IN_PKTS_OK exactly (32,098,518,997 / 32,098,039,780 / 32,098,039,040 /
  32,098,518,997), NOT_VALID / INVALID / LATE / UNCHECKED / NOT_USING_SA / UNUSED_SA 0; XPN 3.21 x 10^10: 7 low-word
  wraps per SA (28 in all) with 0 loss (the 2026-11-22 fix holds under SONiC forwarding).
- **Temperatures:** ASIC 46.2-48.6 C (idle 46.7-47.7), board zone 36-37 C, CPU 40-42 C; fand on its curve (53 % ASIC zone,
  board zone 60 -> 62 %), no failsafe; fans 14,000-17,100 rpm.
- **syncd:** 7.1-9.1 % of one core during the load (cpu-time deltas per interval), RSS 139.4 MB flat (139,432 -> 139,460 kB);
  orchagent 0.4 %. No SAI error from the ring; the only syncd/orchagent warnings were the stop's 4
  `removeBridgePort ... reference count is 1` (sairedis meta check in orchagent: my stop order removed the members before the
  static FDB entries that reference them; `down` then removed the entries; not an SAI failure).
- **Cleanup:** `down` restored CONFIG_DB (INTERFACE mac_addr back, VLANs gone, APPL_DB FDB_TABLE empty); then a reboot. After
  it CONFIG_DB has the same key set as before the test (c0 vs e0), sysctls default, MDIO tool 101.2 s 32/32.
- **`soc_knet_handle_rx: unhandled (type=3, opcode=31)`:** 0 times this session: not at the 12:42Z boot, the two smoke
  processes, the stack start after them, the 30-min load, nor the 13:26Z boot. Still only the one sighting (first Rust syncd
  start after a C++ syncd, G2 session).

## Findings
- **phystat.py with SONiC up and MACsec on collides with syncd's MACsec SA counter reads** (both use the PHY's message
  interface; the PHY tools pause the link polls, not the MACsec sequences). Baseline run 12:50:49-57Z: all four reads OK, but
  syncd logged `external MACsec sequence pn.egress/ingress.0 stopped ... 1.8221 read 0x0000, expected 0x12e3` on all four PHYs
  and `collectData: Failed to get attr of SAI_OBJECT_TYPE_MACSEC_SA` (c2). After the load: phystat failed with `FW Message
  failure` on PHY 0x00 (twice) and 0x17e, while syncd's sequence failed on 0x01 and 0x17f (d5). Both sides recovered at the
  next poll; MKA stayed up, link polls 0 faults. **Not run again**; nothing was retried in a loop.
- **PHY FEC over the run: not established.** Line-side RS-FEC counters (clear on read, cleared by the 12:50:57Z baseline):
  0x01: 0 corrected, 0 uncorrected, 0 symbol errors; 0x17f: 0 corrected, **12 uncorrected**, 0 symbol errors, read during a
  collision and inconsistent with 0 symbol errors, 0 line RFCS on that PHY and exact frame/byte equality on that cable (an
  uncorrectable codeword would cost a frame): treat as a suspect read, not as errors. 0x00, 0x17e: no read (and 0x00's
  corrected/uncorrected registers were consumed by the failed attempt). The baseline (c0) itself showed boot-time counts
  (0x01 12 uncorrected, 0x17e 1 corrected): the counters accumulate from link-up. A FEC read under MACsec needs the MACsec
  counter polling paused (no CLI for it: `counterpoll` lists only PORT_STAT) or the counters exported by syncd.
- SAI `IF_IN_NON_UCAST_PKTS` read ~4000 during the load and dropped back to the control-frame count after the stop: a
  derived counter (difference of two MIB reads taken microseconds apart at 17.7 Mpps), not frames.
- The MDIO tool's summary now equals status.json `seconds` (101.0 = 101.0 s at 12:45Z, 101.2 = 101.2 s at 13:29Z).

## Soak monitor (left running)
`/root/soak-rust/soak.sh` (copy: e3), transient `soak-rust.timer` (`systemd-run --on-calendar='*:0/5'`; gone after a reboot),
appends to `/root/soak-rust/soak-YYYYMMDD.txt` (UTC): system, uptime, link state of the four ports (STATE_DB / APPL_DB),
MACsec SA counts (STATE_DB), port error counters (COUNTERS_DB), syncd / orchagent / wpa_supplicant RSS and CPU time,
temperatures + fand, failed units, journal err / kernel warning / `soc_knet_handle_rx` / duplicate-MAC counts since the last
run. Redis, /proc, /sys, /run and the journal only: no MDIO, no diag command. Stop: `systemctl stop soak-rust.timer`.
The runtime VRFs / addresses of `macsec-traffic-test.sh` (e2) are still on the box, as after earlier sessions.

## Not done / unverified
- PHY FEC and PHY MAC counters for the run (above); PHY die temperature (needs `status dsc`).
- Pause / congestion: none (the ring never oversubscribes an ingress); 4 ports, not 32.

```
86c30d6986d516e866c2a5d2578bbbd4946a481dd6806595fbed62368ad9036c  a0-deploy-boot.txt
26c4f3835d1debd377487c8c8e0d8764c2080a85f2d6754a536e9801293b1eeb  a1-reboot-health-mdio-summary.txt
afc8f82dc1142332b91745efab1f5100ff10d6d94db7fc98174ef7fdcd8e6507  a2-macsec-up-check.txt
83348e72816148ee51bc0fc87c946cf9bc74e88c24bef40466c7edd235fe9c4c  b1-smoke-tables-and-recreate.txt
89499d7bb53c2a54cf1ae627c936bab0bf1854889d9a48b5de52ae1e4e9cf72d  c0-configdb-before-and-checks.txt
5861e52dbfed7c88c1a68d4ac5a1d62bbb0b5fc48515c28f44116db2b41d8ff6  c0-phystat-before.jsonl
7d9b2a47657603f3ec61c91cdae315d7fe53834b316f0e551576afeae8a5994f  c0-sample-idle.txt
16dfccba53011d5958cd1f94ebc8f93adf1cca248ebcc73b421ef86b4714551c  c1-ring-up.txt
66d285e815c9f47a41a3787f423b7a9ba9dec0b7a6f54b55adb0de4de3f8e899  c2-journal-err-phystat-collision.txt
723d7dd3d7e43cef5aba673352b566242e95ee0f9726577ee99a2613b0d2afe6  d0-ring-samples.txt
f1d5008ee10a5b4ea61ed133e43b1c890b3498c7de050e078e2a2d9cd1dfc853  d1-ring-macsec-samples.txt
5861057f260e9bcf4298c2209294b61a974453ebf8bc5f5efcc5556c0069349f  d2-ring-macsec-poststop.txt
40212822a0a0d440eb407dc23abb23c9fbce28152c21e0e3fc5b934c08edda79  d3-ring-asic-full-counters.txt
c9cc2210848ca72dd48e154e0b202a4826f3c375ac4b0f8d23b9bec60a8e8e46  d4-ring-stop-and-journal.txt
571a14ab93bd31ea4eee651a95879e43dc1f0003cd0ae18598bc594f69d1d9eb  d5-phystat-after.txt
3598b3934c5f0f2de62b8c222ea0de3638227d0cfa16da19ae024d7098351fdb  d6-ring-down.txt
488b49dd07924412c7276e43d32c81c20e42f74e781c34575d4befee4e812a81  d7-ring-conservation.json
19d4cb462a64497aa7f0c21f1435ca1efa20955273377922ecd9ac32be5386bc  e0-final-reboot-state.txt
7cbcd19617656bc70aa0f12ea98fcd5823581e34824c67f46efa22f74b7f14bb  e1-final-macsec-up-check.txt
48c384d3c33a7afd0dcdcb2eb27a5c6a9afbb99d4cc10c8889f08f6c6bd83606  e2-final-macsec-traffic-test.txt
6d3b51b56d5945c0b77ec6e6e5edc22b0280b3d44d859b225bb99d4f76cc2d8f  e3-soak-monitor-soak.sh.txt
042b015092b0f997106e7dda5bbc2b34756f703f58e5a37ec1b3c6a497a7a732  e3-soak-monitor-start.txt
```

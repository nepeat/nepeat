# AS7772-32X: load test, PHY-side counters, factory TX taps

2026-11-22. Both loopback DACs (cage 1 <-> 2 = ce0 <-> ce1 = PHY 0x00 <-> 0x01; cage 31 <-> 32 = ce30 <-> ce31 =
PHY 0x17e <-> 0x17f): **26 min at 100 % of line rate in both directions, zero errors**, on the factory TX taps.
Hub: [extphy.md](extphy.md). Times are UTC (the switch clock shows -08:00).

## Result
| | measured |
| --- | --- |
| rate, each of ce0 / ce1 / ce30 / ce31, TX and RX | **96.4 Gb/s of frames, 18.59 Mpps** (SDK 10 s window: 12,054,3xx,xxx B/s, 18,591,5xx pkt/s). With 8 B preamble + the 16 B average IPG that is 100.0 % of 100G; 385.7 Gb/s transmitted by the four ports together |
| duration | ring filled 10:15:47, stopped 10:41:51 (26 min 4 s); 29.10 x 10^9 frames / 18.86 x 10^12 B per port per direction (totals / duration = 18.6 Mpps, 96.5 Gb/s) |
| frame mix (ce0 TX) | 64 B 40.6 %, 65-127 13.5 %, 128-255 13.5 %, 256-511 8.1 %, 512-1023 8.1 %, 1024-1518 13.5 %, 8996 B 2.7 %; mean 648 B |
| loss over the cables | **0**: TPKT and TBYT of each port equal RPKT and RBYT of its cable peer exactly, all four directions (116 x 10^9 frame crossings, 6.0 x 10^14 bits) |
| loss in the switch | **0**: 740 frames injected per ring, 740 discarded per ring at the stop (`RDBGC0` ce0 = ce1 = 740), nothing else; no MMU drop, no pause frame |
| ASIC error counters | none non-zero (no RFCS, RERPKT, RJBR, RFRG, RUND, TERR) |
| PHY RS-FEC (line side, towards the DAC) | **0 corrected, 0 uncorrected codewords, 0 symbol errors** on all four PHYs at every read (5 reads, 10:16 to 10:42); system side CL82 BER count 0; PCS live status unchanged |
| PHY MAC counters | line RPKT of one PHY = line TPKT of its peer, to the frame; RFCS / RJBR / TERR: one each, from the aborted starts (below), unchanged during the soak |
| links | up before and after (ASIC `ps`), PHY PCS live status the same at every read in between; SONiC restored, 6400 pings 0 lost |
| ASIC temperature (hottest monitor) | 46.7 C idle before, **46.2-49.1 C** under load (peak register 51.1 C) |
| PHY die temperature (`status dsc`) | idle 37-39 C, under load **41-42 C** (0x00, 0x01, 0x17e, 0x17f; last read at t + 23 min) |
| board sensors, fans | hottest hwmon reading 44 C (idle 40 C); fand never left its curve: ASIC zone 53 % duty, board zone 55 -> 60 %; 12,900-16,000 rpm, no failsafe during the soak |

Evidence (sha256 prefix): [load-20261122-asic-final.txt](evidence/load-20261122-asic-final.txt) (02c7be5fab8f: all counters after the
stop, `ps`, L2 table), [load-20261122-asic-samples.txt](evidence/load-20261122-asic-samples.txt) (3af33264e348: temperature, fand state,
fan rpm, board sensors every 30 s), [load-20261122-phystat.txt](evidence/load-20261122-phystat.txt) (27560c4216e1),
[load-20261122-phystat-after.jsonl](evidence/load-20261122-phystat-after.jsonl) (e3365709dbdc: PHY counters and taps under SONiC afterwards),
[evora-taps-eye-20261122.txt](evidence/evora-taps-eye-20261122.txt) (016beeb4d2cb: DSC rows and taps of every stage),
[sonic-20261122-after-load-traffic.txt](evidence/sonic-20261122-after-load-traffic.txt) (1b2cd6bb4161).

**What this does not show**
- **SONiC was stopped.** The ring ran on a plain SDK shell with SONiC's port settings (CAUI, autoneg off, IPG 16).
  SONiC's own forwarding (SAI L2/L3) has still only seen pings.
- **No per-interval rate series.** The 30 s sampler asked for `show c nz`, which prints nothing. The rates are two
  SDK 10 s windows at t + 20 s (this run and an aborted start: equal to 5 digits) plus totals over the duration;
  PHY MAC frame counts between reads (18.60 Mpps over 908 s) agree. [loadtest/ring.sh](loadtest/ring.sh) is fixed
  (`show c Changed Same NonZero`), not re-run.
- PHY temperature only at the `status dsc` runs (that command latches the register).
- Bypass mode only; 28 cages without a cable; one DAC type; 26 minutes, not hours.

## How the load is made
[loadtest/ring.sh](loadtest/ring.sh) + [loadtest/bcmd.py](loadtest/bcmd.py), copied to `/run/m-load` on the switch.
Order: `ring.sh up` (stops SONiC), `fwd`, `inject`, `soak`, `stop`, `down` (starts SONiC; PHYs untouched).
- Two 2-port VLANs (100 = ce0 + ce30, 200 = ce1 + ce31, untagged) and four static L2 entries make two
  counter-rotating rings (`ce0 -> cable -> ce1 => ce31 -> cable -> ce30 => ce0` and the reverse). Known unicast
  only: nothing floods, no storm control. The CPU injects 740 frames per ring (seven sizes); a lap takes 1.6 us,
  so the wire holds about 20 kB and the rest queues at the egress ports: every port transmits back to back.
  Same idea as the CS8210 400G loop (`devices/cs8210-32x/scripts/loop400_fwd.py`).
- Stop: the four entries become discard entries; each frame is dropped at its next ingress and counted
  (`RDBGC0`). In the 3 ms between delete and re-add frames flood to the other VLAN member (55-58 k frames on the
  multicast queue of ce30 / ce31: same path, no loss).
- `bcmd.py`: one `bcm.user` behind a unix socket speaking the `openbcm-diag` protocol, shared by ring commands,
  counter reads and the re-host's clause-45 traffic (divisor 53 / delay 10 read back after its `init all`). It
  publishes the ASIC temperature where syncd would, so fand stays on its curve; if it dies fand fails safe.
  `ring.sh soak` samples every 30 s and stops the ring above 95 C or on fand failsafe.

**Two aborted starts, both operator error** (10:12-10:15):
1. The guard stopped the first ring after 30 s: fand was still in its 60 s calm period after the SDK hand-over
   ([load-20261122-guard-abort.txt](evidence/load-20261122-guard-abort.txt), b177da062c38: the guard works).
2. The PHY tool's `close()` sent `exit`, ending the daemon's `bcm.user` with the ring running; the restart's
   `init all` reset the chip under traffic. Fixed (`bcmd.py` refuses `exit`, the backend no longer sends it, `up`
   removes a stale socket).
- The PHY MAC counters picked up **one RFCS / RJBR / TERR each** between 10:14:05 (all 0) and 10:16:18:
  attributed to frames cut by those resets (inferred, not proven). They did not move afterwards.

## PHY-side observability with SONiC running
- `evora-rehost/mdio.py` backend **`diag`**: clause 45 through syncd's diag socket (`openbcm-diag 'phy raw c45
  ...'`), about 6 ms per access, no second SDK process. `rehost.py --backend diag --load-state
  dumps/x/evora-trace/all-cfg.state '<extphy command>'` runs `status`, `status dsc|pmdiag|lts`, `pre`,
  `reg64 get|set|chkecc|chkmiberr`, `info` (all run; `prbs status` only answers with PRBS enabled; `eyescan` and
  `prbs config` not tried). `EVORA_DIAG_SOCKET=/run/m-load/diag.sock` points it at `bcmd.py`. One user at a time.
- **[evora-rehost/phystat.py](evora-rehost/phystat.py)** `[--taps] [--json] <phy ids>`: RS-FEC counters, PCS status,
  both PHY MACs' MIB counters, TX FIR of both sides; about 2 s per PHY.
- `status dsc`: 200 s per PHY (25,000 reads, mostly the microcode event log), **line side only**.

| register (vendor address; `if_side` 0 = line, 1 = system) | what |
| --- | --- |
| 0x4202c352/3, c354/5, c360..c367 (line) | RS-FEC corrected codewords, uncorrected, symbol errors per FEC lane (lo/hi, clear on read). Addresses from the public OpenBCM TSCF-16nm header. **Checked live:** weakening one TX lane (main 30, no post, amp 4) gave 1 corrected, 3 uncorrected, 2 symbol errors on the peer within 4 s, 0 again after restoring |
| 0x4202a082/3 (system) | CL82 BER count (no FEC towards the ASIC) |
| 0x4200d094, d095, d131, d132, d0d2 | TX FIR per lane: pre (low byte) + post1 (high byte), main, post2, post3, amplitude |
| 0x4200d03e | die temperature (410.04 - 0.48705 x value); only refreshed by `status dsc`, reads 0 before the first |
| CLMIB (0x41..) | a full MAC per side, counters not clear on read. `ROVR` counts every frame over 1518 B and still forwards it: a jumbo counter here, not an error (after the test = the number of 8996 B frames) |

## Factory TX taps
- `preem-25G.soc` (run by `rc.soc-100G` after `cfg`): 128 lines `extphy pre <id> <lane> <pre> <main> <post> <post2>
  <post3> <amp>`, **per cage and per lane**: pre 2, main 40-60, post 15-31, post2 4, post3 0, amp 8.
  (`preem-10G.soc` is the 40G set: pre 0, post2 0; not used.)
- `extphy pre` sets the **line side** (towards the cage); the system side keeps the default (read back on all 32
  PHYs). The get form shows lane 0 only. Chip default, both sides: **pre 0, main 43, post 5, amp 15**.
- Before / after on the four cabled PHYs, SONiC idle, same boot (RX eye of the peer, 16 lanes):

  | taps | eye height (mV, up/down) | eye width (mUI, left/right) | RX peaking filter | DFE tap 1 / 2 | FEC corrected |
  | --- | --- | --- | --- | --- | --- |
  | default 0/43/5/15 | 111-141 | 281-359 / 343-433 | 0-3 | 35-43 / 6-10 | 0 |
  | factory | 118-165 | 250-343 / 281-402 | 0 | 21-34 / -8-0 | 0 |
  | factory, ring at line rate | 125-162 | 234-343 / 281-373 | 0 | 22-34 / -8-0 | 0 |

  Taller, a little narrower; receivers turn peaking off and DFE tap 2 goes negative (more equalisation than these
  short DACs need). No link retrained on the change. Neither setting produced a FEC correction: on this cable the
  choice is not measurable in errors.
- **Decision: factory taps on, in the boot script, all 32 PHYs** (the factory state, tuned per lane; the load test
  ran on them). The tap-less script stays beside it as the fallback.

### Boot script with taps
```
T=devices/edge-core-as7772-32x/dumps/x/evora-trace;  R=devices/edge-core-as7772-32x/evora-rehost
# trace (SONiC up, PHYs with default taps): the 128 factory lines, then 'pre <id>' for each PHY
python3 $R/rehost.py --backend diag --load-state $T/all-cfg.state --trace $T/all-pre.trace.txt 'pre 0x0 0 0x2 0x2a 0x13 0x4 0x0 0x8' ...
python3 $R/mkscript.py -o $T/evora-100g-bypass-taps.mdio $T/all-init.trace.txt $T/all-cfg.trace.txt $T/all-pre.trace.txt
$R/install-script.sh $T/evora-100g-bypass-taps.mdio
```
- Installed as `/var/lib/evora-phy/evora-100g-bypass.mdio`: sha256 **e18dddef7c9e...** (21.7 MB, 169 sections);
  trace `all-pre.trace.txt` 5d763a082788. Fallback beside it: `evora-100g-bypass-notaps.mdio` (fdda87ade40b); to
  go back, copy it over the first name and reboot. Both also in `dumps/x/evora-trace/`.
- **Cold power cycle with it (10:00):** 99.4 s, 553,600 writes, 426,459 reads, **0 expected values differed**, 32/32
  verified ([mdio-script-20261122-cold2-taps.txt](evidence/mdio-script-20261122-cold2-taps.txt), b4ee51c42f17);
  line-side taps of all 32 PHYs equal `preem-25G.soc`, system side default
  ([evora-taps-readback-20261122-cold2.txt](evidence/evora-taps-readback-20261122-cold2.txt), 5de61115eceb); 4 ports
  up, 6400 pings 0 lost
  ([sonic-20261122-coldboot3-taps-traffic.txt](evidence/sonic-20261122-coldboot3-taps-traffic.txt), 3bdaf1558a31).
- The tap writes are masked register writes with no waits (+0.4 s at boot).

## Findings and open items
- **IPG 16 costs 0.6 % of throughput:** 96.4 Gb/s with this mix; 97.0 at the standard 12 B (computed, not
  measured). Whether 12 works through the PHY is untested.
- **Frame size:** 8996 B frames (36 Gb/s per port) passed for 26 min. The PHY reports `max_packet=16383`; the
  ASIC ports ran at 9126 (SONiC: 9130). Nothing larger than 8996 B was sent.
- **Pause:** ASIC ports show pause TX RX and the PHY "flow control: enabled", but no pause frame was sent or
  received (counters 0) and the ring never congested an ingress. What the PHY does with a pause frame is
  unknown; a port that receives its peer's pause frames on a loop cable would throttle itself. Test before
  relying on it.
- **Thermal headroom is large at this load** (+2.4 C ASIC, +3 to 5 C PHYs, fans did not move). Four ports are not
  a worst case; 32 loaded ports and MACsec on are untested.
- Not done: a load run through SONiC's forwarding, hours-long soak, `eyescan`, PRBS, live PHY temperature (needs
  the write `status dsc` makes to 0x4200d03d decoded), an exporter for the PHY counters.

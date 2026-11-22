# AS7772-32X: load test, PHY-side counters, factory TX taps

2026-11-22. Both loopback DACs (cage 1 <-> 2 = ce0 <-> ce1 = PHY 0x00 <-> 0x01; cage 31 <-> 32 = ce30 <-> ce31 =
PHY 0x17e <-> 0x17f) ran **26 min at 100 % of line rate in both directions with zero errors**, with the factory TX taps
that the boot script now applies to all 32 PHYs. Background: [extphy.md](extphy.md), [sonic.md](sonic.md),
[evora-rehost.md](evora-rehost.md). Times are UTC (the switch clock shows -08:00).

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

Evidence (sha256 prefix): [load-20261122-asic-final.txt](load-20261122-asic-final.txt) (02c7be5fab8f: all counters after the
stop, `ps`, L2 table), [load-20261122-asic-samples.txt](load-20261122-asic-samples.txt) (3af33264e348: temperature, fand state,
fan rpm, board sensors every 30 s), [load-20261122-phystat.txt](load-20261122-phystat.txt) (27560c4216e1),
[load-20261122-phystat-after.jsonl](load-20261122-phystat-after.jsonl) (e3365709dbdc: PHY counters and taps under SONiC afterwards),
[evora-taps-eye-20261122.txt](evora-taps-eye-20261122.txt) (016beeb4d2cb: DSC rows and taps of every stage),
[sonic-20261122-after-load-traffic.txt](sonic-20261122-after-load-traffic.txt) (1b2cd6bb4161).

**What this does not show**
- **SONiC was stopped for the load.** The ring ran on a plain SDK shell (`bcm.user`, `init all`, the board's config.bcm,
  SONiC's port settings: CAUI, autoneg off, average IPG 16). SONiC's own forwarding (SAI L2/L3) has still only seen pings.
- **No per-interval rate series.** The 30 s sampler asked for `show c nz`, which prints nothing; the rates are two SDK
  10 s windows at t + 20 s (in this run and in an aborted start: the same to 5 digits) plus totals over the duration,
  and the PHY MAC frame counts between reads (18.60 Mpps over 908 s) agree. [loadtest/ring.sh](loadtest/ring.sh) is fixed
  (`show c Changed Same NonZero`), not re-run.
- PHY temperature is only known at the `status dsc` runs (the register is latched by that command, not live).
- Bypass mode only; the other 28 cages have no cable; one DAC type; 26 minutes, not hours.

## How the load is made
[loadtest/ring.sh](loadtest/ring.sh) + [loadtest/bcmd.py](loadtest/bcmd.py), copied to `/run/m-load` on the switch (runtime only).
- Two 2-port VLANs (100 = ce0 + ce30, 200 = ce1 + ce31, untagged) and four static L2 entries make two counter-rotating
  rings: `ce0 -> cable -> ce1 => ce31 -> cable -> ce30 => ce0` and the reverse. Known unicast only: nothing floods, so
  no storm control and none of the MAC-loopback throttling of the runbook gotcha. The CPU injects 740 frames per ring
  (`tx`, seven sizes); one lap takes 1.6 us, so the wire holds about 20 kB and the rest queues at the two egress ports:
  every port transmits back to back. Same idea as the CS8210 400G loop (`devices/cs8210-32x/scripts/loop400_fwd.py`).
- Stop: the four entries become discard entries; each frame is dropped at its next ingress and counted (`RDBGC0`).
  During the 3 ms between delete and re-add the frames flood to the one other VLAN member (55-58 k frames on the
  multicast queue of ce30 / ce31: same path, no loss).
- `bcmd.py` keeps one `bcm.user` behind a unix socket that speaks the `openbcm-diag` protocol, so ring commands, counter
  reads and the PHY re-host's clause-45 traffic share one serialised SDK process (MDIO divisor 53 / delay 10 read back
  after its `init all`). It publishes the ASIC temperature where syncd would, so fand stays on its normal curve; if it
  dies the file goes stale and fand fails safe. `ring.sh soak` samples every 30 s and stops the ring above 95 C or on
  fand failsafe.
- Order: `ring.sh up` (stops SONiC), `fwd`, `inject`, `soak`, ..., `stop`, `down` (starts SONiC; PHYs untouched).

**Two aborted starts, both mine** (10:12-10:15): (1) the guard stopped the first ring after 30 s because fand was still
in the 60 s calm period after the SDK hand-over ([load-20261122-guard-abort.txt](load-20261122-guard-abort.txt), b177da062c38:
the guard works); (2) the PHY tool's `close()` sent `exit`, which ended the daemon's `bcm.user` with the ring running;
the restart's `init all` reset the chip under traffic. Fixed (`bcmd.py` refuses `exit`, the backend no longer sends it,
`up` removes a stale socket). The PHY MAC counters picked up **one RFCS / RJBR / TERR each** between 10:14:05 (all 0)
and 10:16:18; I attribute them to frames cut by those resets (inferred, not proven). They did not move afterwards.

## PHY-side observability without stopping SONiC
- `evora-rehost/mdio.py` has a third backend, **`diag`**: clause 45 through syncd's diag socket (`openbcm-diag 'phy raw
  c45 ...'`), about 6 ms per access, no second SDK process. `rehost.py --backend diag --load-state
  dumps/x/evora-trace/all-cfg.state '<extphy command>'` runs the vendor `status`, `status dsc|pmdiag|lts`, `pre`,
  `reg64 get|set|chkecc|chkmiberr`, `info` with SONiC up (all run; `prbs status` only answers with PRBS enabled;
  `eyescan` and `prbs config` not tried). `EVORA_DIAG_SOCKET=/run/m-load/diag.sock` points it at `bcmd.py` instead.
  One user at a time: the library's register accesses are multi-step.
- **[evora-rehost/phystat.py](evora-rehost/phystat.py)** `[--taps] [--json] <phy ids>`: RS-FEC counters, PCS status, both
  PHY MACs' MIB counters and the TX FIR of both sides, about 2 s per PHY.
- `status dsc` takes 200 s per PHY (25,000 reads; most of it is the microcode event log) and shows the **line side only**.
- Register facts found (vendor addresses as `status pmdiag` prints them; `if_side` 0 = line, 1 = system):
  - RS-FEC, line side: corrected codewords 0x4202c352/3, uncorrected c354/5, symbol errors per FEC lane c360..c367
    (lo/hi words, clear on read). Addresses from the public OpenBCM TSCF-16nm header (the Evora PCS matches that
    layout, not the Tomahawk one). **Checked live:** weakening one TX lane (main 30, no post, amp 4) gave 1 corrected,
    3 uncorrected, 2 symbol errors on that lane's peer within 4 s and 0 again after restoring the taps.
  - System side (no FEC towards the ASIC): CL82 BER count 0x4202a082/3.
  - TX FIR, per lane: 0x4200d094 (pre low byte, post1 high byte), d095 main, d131 post2, d132 post3, d0d2 amplitude.
  - Die temperature 0x4200d03e (410.04 - 0.48705 x value): only refreshed by `status dsc`; reads 0 before the first one.
  - The PHY has a full MAC per side (CLMIB counters, not clear on read). Its `ROVR` counts every frame over 1518 B
    and still forwards it: after the test ROVR equals the number of 8996 B frames. It is a jumbo counter here, not an error.

## Factory TX taps
- `preem-25G.soc` (run by `rc.soc-100G` after `cfg`) is 128 lines `extphy pre <id> <lane> <pre> <main> <post> <post2>
  <post3> <amp>`: one per lane of all 32 PHYs, **per cage and per lane**: pre 2, main 40-60, post 15-31, post2 4,
  post3 0, amp 8. (`preem-10G.soc` is the 40G set: pre 0, post2 0; not used.)
- `extphy pre` addresses the **line side** (towards the cage): after a set the line-side registers hold the values and
  the system side keeps the default on every lane (read back on all 32 PHYs). The get form shows lane 0 only.
- Without them every lane of both sides runs the chip default **pre 0, main 43, post 5, amp 15**.
- Before / after on the four cabled PHYs, SONiC idle, same boot (RX eye of the peer, 16 lanes):

  | taps | eye height (mV, up/down) | eye width (mUI, left/right) | RX peaking filter | DFE tap 1 / 2 | FEC corrected |
  | --- | --- | --- | --- | --- | --- |
  | default 0/43/5/15 | 111-141 | 281-359 / 343-433 | 0-3 | 35-43 / 6-10 | 0 |
  | factory | 118-165 | 250-343 / 281-402 | 0 | 21-34 / -8-0 | 0 |
  | factory, ring at line rate | 125-162 | 234-343 / 281-373 | 0 | 22-34 / -8-0 | 0 |

  Taller and a little narrower; the receivers turn their peaking off and DFE tap 2 goes negative, i.e. the factory
  setting equalises more than these short DACs need. No link retrained when the taps changed. Neither setting produced
  a single FEC correction, so on this cable the choice is not measurable in errors.
- **Decision: factory taps on, in the boot script, all 32 PHYs.** They are the factory state, tuned per lane for this
  board, and the load test above ran on them. The old script stays beside the new one as the fallback.

### Boot script with taps
```
T=devices/edge-core-as7772-32x/dumps/x/evora-trace;  R=devices/edge-core-as7772-32x/evora-rehost
# trace (SONiC up, PHYs with default taps): the 128 factory lines, then 'pre <id>' for each PHY
python3 $R/rehost.py --backend diag --load-state $T/all-cfg.state --trace $T/all-pre.trace.txt 'pre 0x0 0 0x2 0x2a 0x13 0x4 0x0 0x8' ...
python3 $R/mkscript.py -o $T/evora-100g-bypass-taps.mdio $T/all-init.trace.txt $T/all-cfg.trace.txt $T/all-pre.trace.txt
$R/install-script.sh $T/evora-100g-bypass-taps.mdio
```
- Installed on the switch as `/var/lib/evora-phy/evora-100g-bypass.mdio`: sha256 **e18dddef7c9e...** (21.7 MB, 169 sections);
  trace `all-pre.trace.txt` 5d763a082788. Fallback beside it: `evora-100g-bypass-notaps.mdio` (fdda87ade40b, the
  script of sonic.md); to go back, copy it over the first name and reboot. Both also in `dumps/x/evora-trace/`.
- **Cold power cycle with the new script (10:00):** 99.4 s, 553,600 writes, 426,459 reads, **0 expected values
  differed**, 32/32 verified ([mdio-script-20261122-cold2-taps.txt](mdio-script-20261122-cold2-taps.txt), b4ee51c42f17);
  line-side taps of all 32 PHYs read back equal to `preem-25G.soc`, system side default
  ([evora-taps-readback-20261122-cold2.txt](evora-taps-readback-20261122-cold2.txt), 5de61115eceb); 4 ports up, 6400 pings
  0 lost ([sonic-20261122-coldboot3-taps-traffic.txt](sonic-20261122-coldboot3-taps-traffic.txt), 3bdaf1558a31).
- The tap writes are masked register writes with no waits (0.3 s per lane through the re-host, +0.4 s at boot).

## Findings and open items
- **IPG 16 costs 0.6 % of throughput:** this mix carries 96.4 Gb/s; at the standard 12 B it would carry 97.0 (computed,
  not measured). The factory uses 16 in bypass and 48 with MACsec on; whether 12 works through the PHY is untested.
- **Frame size:** 8996 B frames (36 Gb/s of each port's traffic) passed for 26 min without a drop. The PHY reports
  `max_packet=16383`; the ASIC ports ran at 9126 (SONiC: 9130). Nothing larger than 8996 B was sent.
- **Pause:** the ASIC ports show pause TX RX and the PHY reports "flow control: enabled", but no pause frame was sent
  or received by either (ASIC and PHY MAC counters 0) and the ring never congested an ingress. What the PHY does with
  a pause frame (pass, absorb, act) is unknown. A port that receives its peer's pause frames on a loop cable would
  throttle itself: test before relying on it.
- **Thermal headroom is large at this load:** +2.4 C on the ASIC, +3 to 5 C on the PHYs, fans did not move. Four
  ports are not a worst case for the ASIC; 32 loaded ports and MACsec on are untested.
- Not done: a load run through SONiC's own forwarding, hours-long soak, `eyescan`, PRBS, live PHY temperature
  (needs the write `status dsc` makes to 0x4200d03d decoded), an exporter for the PHY counters.
- For the devshell: `python3Packages.unicorn` is still fetched with `nix shell` for every re-host run.

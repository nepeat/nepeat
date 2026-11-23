# AS7772-32X: frames lost under encrypted load (found and fixed)

2026-11-22. **Cause: the PHY's ingress MACsec engine refused every frame whose 32-bit PN field was 0** (its "PN is not
zero" SecTAG check, on by default). With XPN that field is only the low half of the packet number and is 0 once every
2^32 frames: **one frame lost per SA per 2^32 frames, 4 min 2 s apart at 100G** with the load test's frame mix.
**Ours** (our table left a chip default in place that is wrong for XPN; the vendor flow never touches it), fixed in
the board table, proven by test. Hub: [macsec.md](macsec.md#under-sonic); load method: [load-test.md](load-test.md).

## The defect, explained
- The run of [evidence/macsec-load-20261122.txt](evidence/macsec-load-20261122.txt): 560 s at 17.75 Mpps = 9.9 x 10^9 frames
  per SA = 2.3 x 2^32: **2 wraps of the low word, 2 frames lost on each of the 4 cable directions** (TPKT - peer RPKT = 2,
  four times) = 4 of 740 per ring. The 20 s and 90 s runs after it continued on the same SAs from 9.95 to 11.9 x 10^9
  (2.3 -> 2.8 x 2^32): no wrap, no loss. (That they used the same SAs is read from the evidence file's order, not logged.)
- Not the cause, each ruled out by a run below: `phystat.py` reads during traffic (the earlier suspect), the stop of
  the ring / how the 740 are counted, L2 learning or SONiC leftovers, SA lifetime or MKA being absent, FEC or the cable.
  Bypass never showed it because nothing parses a SecTAG there.

## Method
1. **Reproduce without waiting for 2^32 frames** ([macsec-wrap-test.sh](macsec-wrap-test.sh), on the switch, SONiC up):
   static XPN-256 SAs on cages 1 <-> 2 through `openbcm-diag extmacsec run` (the table and executor SONiC uses) with
   a chosen first packet number, 600 pings across it, every SecY counter and the ASIC counters.
2. **Find the register.** `in_pkts_bad_tag` is the ingress transform engine's (EIP-164). The vendor library's
   `secy_rules_sectag_update` (dispatch slot 0x12dc3710, `(access, 7 flag bytes + EtherType)`), run on the mock with
   one flag at a time, writes **ingress transform register 0x5000** (library address 0x44005000, channel 0): bits 0-6 in
   the order of the public header's fields (compare EtherType, check V, KaY, C/E, SC, SL, **PN**), EtherType in the
   high half. The vendor init never writes it (decoded `b0-init2` trace); live it read **0xe588007f** on every PHY
   looked at (reset value: all seven checks on). Bit names are the public header's order, not documented for this chip;
   that bit 6 is the PN check is shown by the test.
3. **Fix** in `nix/modules/boards/as7772/ext-macsec.nix`: the 32 `sa-add-first.ingress.*` sequences end with one LMI write
   of 0x5000: **0xe588003f for the XPN suites**, 0xe588007f (the default, PN 0 stays invalid as 802.1AE asks) for the
   32-bit ones. Table hash re-pinned (1ef74ead737f...); `evora-rehost/seqcheck.py` knows the one write that no
   recording has and still finds everything else identical to the five recordings
   ([evidence/macsec-seqcheck-20261122.txt](evidence/macsec-seqcheck-20261122.txt)). System `wchbsjy4l9al...`: its
   closure differs from `gcyrw820...` in the table and what refers to it (sai.profile, the syncd unit, etc) only.
4. **Prove at rate:** the ring of load-test.md on SAs that SONiC's MKA made with the fixed table, long enough for
   8 wraps, no PHY access during the run; then the same again with PHY reads every 30 s.

## Runs
| run | setup | frames per SA | low-word wraps per SA | lost | where |
| --- | --- | --- | --- | --- | --- |
| earlier, before the fix | ring, MACsec, IPG 48, 560 s, 3 PHY reads | 9.9 x 10^9 | 2 | **2 per SA** (4 of 740 per ring) | ASIC TPKT - peer RPKT; PHY line TX = peer line RX |
| wrap test, before | static SAs, first PN 2^32 - 256, then 2 x 2^32 - 256; 600 pings each | 600 | 1 each | **1 per SA per wrap**: exactly the frame with PN field 0 (ping 256; its reply's slot costs ping 257) | receiving PHY: `in_pkts_bad_tag` 1, `in_pkts_ok` = sender's encrypted - 1; ASIC RPKT one short, no RFCS |
| wrap test, before, controls | first PN 1; first PN 2^31 - 256 | 600 | 0 | 0 | |
| wrap test, **after** | the same two wraps, and PN 1 | 600 | 1 each | **0**, `in_pkts_bad_tag` 0, encrypted = accepted | |
| (a) ring after, **no PHY access during the run** | MACsec by SONiC (MKA, XPN-256), IPG 48, 2080 s, 17.75 Mpps | 36.92 x 10^9 | 8 | **0** (740 of 740 per ring) | every hop equal to the frame: ASIC TX = PHY sys RX = line TX = peer line RX = peer sys TX = ASIC RX; +32.000 B per frame on the line |
| (b) ring after, **phystat.py every 30 s** | same SAs continued, 819 s, 26 reads of each PHY (FEC, PCS, both MACs' MIB, through LMI) | 14.55 x 10^9 | 3 | **0** | as (a); FEC corrected / uncorrected 0 at every read |
- After the fix: **2.06 x 10^11 encrypted frame crossings (4 SAs x 51.5 x 10^9), 44 wraps, 0 lost**; ASIC 45.2-49.6 C,
  fans on their curve, guard never fired. Before: 12 wraps seen, 12 frames lost.
- Not run, because the cause was found: bypass control (c), frame-size runs (d), the hours-long bound (e). Run (a)
  is 35 min, not hours: it bounds any *other* loss at < 1.5 x 10^-11 per frame (0 in 2 x 10^11, 95 %).
- The 30 s sampler of `loadtest/ring.sh` works now (counters and rates per sample; it printed none on 2026-11-22 morning).
- Evidence (sha256 prefix): [wrap test before](evidence/macsec-loss-wrap-before-fix-20261122.txt) (068be0a9d340),
  [after](evidence/macsec-loss-wrap-after-fix-20261122.txt) (c71d4970c207),
  [soak (a) + (b)](evidence/macsec-loss-soak-20261122.txt) (d9ee0a0a1938), samples
  [a](evidence/macsec-loss-soak-a-samples-20261122.txt) (c7baa6a675ab) / [b](evidence/macsec-loss-soak-b-samples-20261122.txt) (bd499e86b0e4),
  [PHY reads](evidence/macsec-loss-soak-phystat-20261122.jsonl) (4e22589847d5),
  [SONiC back, traffic test](evidence/macsec-s3-traffic-after-loss-fix-20261122.txt) (bb0577bf82de: 6,400 pings 0 lost,
  encrypted = accepted, 0 failed units). Test SA parameter files (keys): `dumps/x/taskU/`, removed from the switch.

## Not verified, and what else turned up
- **Vendor mode 2 / `macsec.py`-installed SAs:** not run. They use the same engine with the same default, so an XPN SA
  made by `macsec.py` loses the same frame (inferred; `macsec.py` has no call for the fix yet:
  `secy_rules_sectag_update` with byte 6 = 0 is the library's way). The vendor's own mode 2 is 32-bit PN with
  "roll-over allowed": inferred to lose the PN 0 frame at each roll-over too.
- The fix was exercised on cages 1, 2, 31, 32 with XPN-256 only. The 0x7f write for the 32-bit suites is the value all
  earlier tests ran with, but that sequence was not run on the switch. Break-out channels 1-3 (0x5040..) are not written.
- Read back after the last stack start: 0x44005000 = 0xe588003f on the four PHYs; an idle PHY (0x02) keeps 0x7f.
- No cold boot was done with the new system (the write is in the SA sequence, so it follows every MKA; a stack
  restart and a syncd-only restart were done).
- **MKA after a stack restart needed a macsecmgrd restart both times today** (deploy's syncd restart 15:29; `sonic.target`
  start 16:26): "duplicated SCI" / "duplicate MAC detected", both ends with SCI ...d5 0001, i.e. wpa_supplicant started
  before the far ends had their MAC. The wait added for this (macsec.md) did not cover these two starts.
  `macsec-traffic-test.sh` restarts macsecmgrd itself, which hides it. Not touched (not this task's files).
- A PN threshold / rekey before 2^32 is not needed for this and would not have helped: the frame is dropped by a
  tag check, not by SA state.

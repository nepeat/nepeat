# AS7772 G8: Y1's MDIO arbitration fixes (7aba11f) on the hardware, the reworked test scripts, rx_dropped, five reboots (agent H6, 2026-11-24 UTC)

Boot default and final system: **7aba11f #as7772** = `/nix/store/ahzyb6kkpyy4rb8pkps1j2y841kcs4cq-nixos-system-as7772-26.11.20261111.d4f25a3` (generation **33**; `DEPLOY_REV=7aba11f deploy.sh boot`).
Rollback: generation **32** = `/nix/store/azkkiphrpmml34ml3vi52rffc7m2nxr9-nixos-system-as7772-26.11.20261111.d4f25a3` (3c84eb3), still in the profile list.
libsai `9ykrpgxm...-sai-openbcm-rs/lib/libsai.so.1.0.0`. Box clock is PST in transcripts; times here UTC. No PHY hang, no power cycle,
no MDIO tool, no NOR/U-Boot/CPLD write. Fan driver bound and fand without failsafe after every boot. Lab CAK, the wrap test's parameter-file
keys, 64-hex strings and sak/cak/auth_key values grepped in every file here: 0 hits (the only 64-hex strings are the script sha256 sums in b4).
Helpers (not in the repo, text in z0): /root/h6-state.sh, h6-diagpoll.sh, h6-sa-sampler.sh, h6-ring5.sh on the switch, h6-phystat-loop.sh on the
workstation; G7's /root/h5-lat.sh for e2. phystat.py ran from the working tree, whose device directory is identical to 7aba11f (git diff empty).

| file | UTC | what | result |
| --- | --- | --- | --- |
| a0-baseline.txt | 2026-11-24T03:14:11Z | baseline gen 32 | gen 32 azkkiphr booted; as7760_fan bound (hwmon7), fand active, failsafe False; 0 failed; 8 SAs; ring check OK |
| b1-closure-diff.txt | 2026-11-24T03:14:11Z | closure diff gen 32 -> 7aba11f | only sai-openbcm-rs (libsai.so, openbcm-asic-temp, +111.5 KiB) and sonic-nix.py (per-PID temp file) changed; the rest embeds their paths; kernel, modules (fan driver), initrd identical; fand absent from the diff; ext tables byte-identical: PASS |
| b2-deploy-boot.txt | 2026-11-24T03:14:50Z | deploy.sh boot | DEPLOY_REV=7aba11f deploy.sh boot rc 0; system-33 = ahzyb6kk default; boot.scr on the SSD == gen 33 |
| b3-reboot-postboot.txt | 2026-11-24T03:18:03Z | reboot into gen 33 | ahzyb6kk booted; fan bound, no failsafe; 0 failed (sonic-nix-apply success, 0 fields); 32/32 PHYs 101.3 s; libsai 9ykrpgxm; extphy U 0 D 0 X 0, gate max 345 us; up-check PASS 16.3 s |
| b4-scripts-copy.txt | 2026-11-24T03:18:24Z | script copy 7aba11f -> /root | 11 scripts from 7aba11f to /root, sha256 equal |
| c0-phystat-smoke.txt | 2026-11-24T03:19:13Z | phystat smoke after boot, temp prints - | one phystat run rc 0, 12.9 s; temp prints - ; first read after boot: uncorrected 3 on 0x001 / 0x17f (since bring-up) |
| d1-diagpoll.txt | 2026-11-24T03:48:01Z | extmacsec poller samples | d1 extmacsec poll: 1498 calls, 0 failed, max 105.8 ms |
| d1-phystat.jsonl | 2026-11-24T03:48:01Z | phystat JSON lines of step 2 | phystat JSON of d1 (loop A 41 runs, B 24) |
| d1-rekey-phystat-loop.txt | 2026-11-24T03:48:00Z | step 2: rekey 20/230 + 65 phystat runs + 1 s extmacsec poll | attempt 1 (superseded by d2): 65 phystat runs rc 0 + 1 KILLED by the agent (REPL limit) -> its lease expired (1054 ms, the dead-client bound); rekey 0 lost, 168 installs, restored; rc not captured |
| d1-sa-samples.txt | 2026-11-24T03:48:01Z | COUNTERS_DB SA samples | d1 COUNTERS_DB SA samples |
| d2-diagpoll.txt | 2026-11-24T04:07:47Z | extmacsec poller d2 | d2 extmacsec poll: 1035 calls, 0 failed, median 16.1 / max 110.2 ms |
| d2-phystat.jsonl | 2026-11-24T04:07:47Z | phystat JSON of d2 | phystat JSON of d2 (66 runs x 4 PHYs; temp_c null, FEC 0/0) |
| d2-rekey-phystat-loop-clean.txt | 2026-11-24T04:07:47Z | step 2 clean repeat: rekey 20/230 + 66 phystat runs + 1 s extmacsec poll | rekey 20 s / 230 s rc 0 + 66/66 phystat runs rc 0 + extmacsec every 1 s: expired +0, begun = ended (+15048), waits under the SAI lock 0, SAI calls waited outside the lock +367 (journal 366, median 39 / max 72 ms), 0 stopped / read 0x0000 / collectData / poll fault / ERROR; 0 of 483000 pings lost; 168 installs: PASS |
| d2-sa-samples.txt | 2026-11-24T04:07:47Z | SA samples d2 | d2 COUNTERS_DB SA samples: rise; every decrease is a new SA under a reused name |
| e1-gate.txt | 2026-11-24T04:14:03Z | step 3: gate after step 2 and after 5 idle min | gate after d2 and after 5 idle min: W 0 (never waited), >2 ms 0, max 1085 us; behind a library run 132 -> 208 (rekey) -> 228 (idle): PASS |
| e2-link-latency-5.txt | 2026-11-24T04:14:03Z | step 3: 5 link-down cycles, MACsec on | 5 cycles MACsec on, no diag use: 173 / 196 / 374 / 420 / 456 ms (mean 324), no busy answer during the arm: PASS |
| f1-macsec-traffic-test.txt | 2026-11-24T04:16:24Z | macsec-traffic-test | 0 lost; encrypted = accepted (3 SCs); 0 ingress errors; RESTORED and verified; rc 0; ring check OK |
| f2-sonic-traffic-test.txt | 2026-11-24T04:18:21Z | sonic-traffic-test | 0 lost (6400 pings); RESTORED and verified; rc 0; ring check OK |
| f3-macsec-rekey-test.txt | 2026-11-24T04:26:52Z | macsec-rekey-test 20/100 | rekey 20 s / 100 s: 0 of 210000 pings lost; 72 installs; RESTORED and verified; rc 0; ring check OK |
| f4-macsec-wrongcak-test.txt | 2026-11-24T04:29:00Z | macsec-wrongcak-test | wrong CAK: cage 1-2 100 % loss, ICV mismatch x20, 31-32 0 %; right profile back 0 %; RESTORED and verified; rc 0; ring check OK |
| f5-macsec-wrap-test.txt | 2026-11-24T04:31:25Z | macsec-wrap-test one w1 w2 | sets one / w1 (2^32-256) / w2 (2x2^32-256): 600/600 each, in_pkts_bad_tag 0; AVERAGE_IPG as before; RESTORED and verified; rc 0; ring check OK |
| f6-macsec-clear-window-test.txt | 2026-11-24T04:33:04Z | macsec-clear-window-test (sonic.target restart) | sonic.target restart: gate 0 frames with engines on and no SA; in clear (bypass) 9 + 78 replies (known open item); up-check PASS (rc 0); RESTORED and verified; ring check OK |
| f7-rekey-sigint.txt | 2026-11-24T04:36:27Z | SIGINT mid-run of macsec-rekey-test + cleanup dry-run | SIGINT 75 s in: restore ran, RESTORED and verified, rc 130 (real); no processes or snapshot left; --cleanup --dry-run: no writes; up-check -s macsecmgrd PASS; ring check OK |
| g1-ring-rx-dropped.txt | 2026-11-24T04:42:28Z | step 5: rx_dropped classification during sonic-ring | ring 3 min 40 s; CPU got 94 frames on Ethernet0 in 120 s: 59 EAPOL + 35 LLDP; rx_dropped +35 = LLDP count; down RESTORED and verified; check OK |
| g2-ring-phystat.txt | 2026-11-24T04:42:29Z | step 5: phystat during the ring, temp_c missing | phystat during the ring: temp prints - (null); read 2 FEC 0/0/0; read 1 counts are link relocks of steps 3-4 |
| h1-five-reboots.txt | 2026-11-24T04:57:42Z | step 6: five warm reboots | 5/5 warm reboots: gen 33, fan bound, no failsafe, 0 failed, sonic-nix-apply success (0 tracebacks), 32/32 PHYs 100.6-102.1 s, up-check PASS 15.2-16.5 s |
| i1-final-macsec-traffic-test.txt | 2026-11-24T04:59:52Z | final macsec-traffic-test | final: encrypted = accepted, 0 errors, RESTORED and verified, rc 0; ring check OK |
| i2-final-state.txt | 2026-11-24T04:59:53Z | final state, generations 32 / 33 | end state: gen 33 ahzyb6kk booted and default; fan bound; 0 failed; generation 32 still listed |
| z0-helpers.txt | 2026-11-24T05:00:04Z | helper scripts of this session (text) | text of the session helpers |

## Findings
- **Lease starvation fixed** (d2): rekey with 66 back-to-back phystat runs and an extmacsec poll every second: 0 expired leases (G7: 2 of 19
  runs), waits under the SAI lock 0, 367 SAI calls waited outside it (median 39 ms, max 72 ms; G7's SA sequences waited 1001 / 1007 ms),
  extmacsec answered in 14-110 ms throughout (no stall), 0 pings lost.
- **Gate never waits** (b3, e1, i2): "waits for a library run before a poll" 0 on every boot and under the rekey (G7: 72-89 ms on every boot);
  longest gate time per poll 1085 us; busy answers counted during SA installs and stats runs (behind a library run 10 at boot, +76 during d2).
- Link-down latency unchanged in shape (e2, 5 cycles, 173-456 ms; G7 20 cycles 41-477 ms).
- **Scripts:** all six end "RESTORED and verified" with exit 0 under `bash -l` (the exit status is now the real one: SIGINT gives 130),
  ring check OK after each; SIGINT mid-run restores by itself, --cleanup --dry-run then finds nothing to do (f7).
- **rx_dropped** on Ethernet0 during the ring = the LLDP frames exactly (35 = 35 in 120 s); EAPOL (59) is not counted; nothing else reaches the
  CPU (g1). Why the kernel counts LLDP as dropped is inferred (no protocol handler after lldpd's packet tap), not proven.
- **temp_c** now null / "-" instead of 410.0 (c0, d2, g2).
- **sonic-nix-apply** succeeded on all six boots of the new build (b3, h1), 0 tracebacks (G7 g1: FileNotFoundError at boot).
- FEC: the first read after a quiet period counts link relocks (c0: 3 uncorrected on 0x001 / 0x17f after the boot; g2 read 1: 9 / 3 / 6 / 3
  after steps 3-4's admin cycles and stack restart); under load 0 / 0 (g2 read 2, d2 every read). The per-relock attribution is inferred.
- **Process note (d1):** the first step-2 attempt was cut by the agent's REPL wall-clock limit: one phystat client was killed inside a
  transaction (its lease expired after 1054 ms, as documented for a dead client) and the rekey test's rc was lost (it finished unattached,
  restored and verified). d2 repeats the step cleanly and is the result.
- Clear-window test (f6): frames still pass in clear for a few seconds around a stack restart (known open item, macsec.md), the gate held (0).

```
72ea9e4255d81af7f1d26e1b521e19f49723ad16ab38a8bbebd04d428029d442  a0-baseline.txt
e8e0d7abe0731bedd1c10566e92c347a0b145310ad81ddc0b8790f0c90915adb  b1-closure-diff.txt
90af0acc36d27ddca12b5e9e2ce79321ba6bb1015f2bd9dc7268e9fa9ea6224a  b2-deploy-boot.txt
55fbd30005becc8763f6282b6364d5ced68e24f49ebd23183aa4144c123dba1d  b3-reboot-postboot.txt
97ac21e1ec20e8aa5da4029283328e4d15718d24bbd066384869190ce1077af2  b4-scripts-copy.txt
2feed591e960709761855ac7fa791fbb355656306ba19dad458a4bfd3ed545dd  c0-phystat-smoke.txt
39e32279448cbfb407777694bf8651f3da71c9471287eeb885fa8a7548683d07  d1-diagpoll.txt
6b1750487d5980dd1b645a7f3f67838287874ee8fc0fbb0504922cd286b65d19  d1-phystat.jsonl
ba584742c4655f0721b6fbc9cee68b0f3f5e0333a0ac2633216db863fbde6ad0  d1-rekey-phystat-loop.txt
1f2d7f4156da998dd88055184d4e38601f3eb285b78469ed30afcf141b8758a9  d1-sa-samples.txt
a9b602a567eb05628490c2879dcae7d5183c6fecae1762f6ba1df43d1a784344  d2-diagpoll.txt
7165540c712cfd428d825f2a9e370f95fb9a876a672b8e07fd50354179c55521  d2-phystat.jsonl
6671943cffee894058d274c353418519a06e7805c8d9d280b0d5e8f7e7451b83  d2-rekey-phystat-loop-clean.txt
2ba6450cb09856c2ca8e1d8f691911eb19af6107237837abeeafaa9e933cf95e  d2-sa-samples.txt
b01294da1fa1d077d5126d86aa42e4932a15530d62550f65dc7e2dcb8b68e8e2  e1-gate.txt
b4785d880660253d5f0aed031d0db6ff2f08afc7d30b66cf7f58346f54fac435  e2-link-latency-5.txt
a99c7f49081b6a05774229aad1891792039ccf1235443d9e751a651e18d605fe  f1-macsec-traffic-test.txt
01a5a4938288b1b68a1e4032474d344104386421c37b185ea4a3ef5a747e08f4  f2-sonic-traffic-test.txt
4975a7c3236a9a44677f49a3e0002511d4b18d7c1154e276be48036dcd2c4dcf  f3-macsec-rekey-test.txt
71e78ed99009e660eda0cb84073e1c8ad1793092194566e3b8df0d91558e6736  f4-macsec-wrongcak-test.txt
9a298323ad97d3c57b7599685636e9a9249b66ae95a1decd77422a27ed26476f  f5-macsec-wrap-test.txt
8b5818b327fdc53981d1f1f9fa317c8bdbcdf94df11b98b903c6346f3c7c510a  f6-macsec-clear-window-test.txt
b0629b4ded29d11ca8b00247fd447ad2c339373deddb33e4d8c4397f6783fbdd  f7-rekey-sigint.txt
b5f236e613db08c5fff5bc96af0623f2bc6d22739a3144d6ed2f126899fff7fe  g1-ring-rx-dropped.txt
622b41fc5982e4829828269d2cd9464f1ed3bd3aed3b684cff4dd19d7eeaceb7  g2-ring-phystat.txt
819d0d89398ff0955554bf72ba1bedd44e04d86659d09ae1ae40d6bacb02453c  h1-five-reboots.txt
3a559d6378389cfb74cf7afcfee42a2c39cc2d493a289d04b9ad9061370df717  i1-final-macsec-traffic-test.txt
ef38f344907b9b0f7441856f5c716d0a55317e332aca7dd149ec438b87708699  i2-final-state.txt
8ac4c6fcd4dd4f45f04c269a5f4a10e56766c54acca97914a56c4c3b3d96dd48  z0-helpers.txt
```

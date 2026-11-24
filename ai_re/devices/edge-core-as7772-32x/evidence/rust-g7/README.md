# AS7772 G7: per-PHY MDIO arbitration (3c84eb3) on the hardware, ring with PHY FEC sampling, link-down latency (agent H5, 2026-11-23 UTC)

Boot default and final system: **3c84eb3 #as7772** = `/nix/store/azkkiphrpmml34ml3vi52rffc7m2nxr9-nixos-system-as7772-26.11.20261111.d4f25a3` (generation **32**; `DEPLOY_REV=3c84eb3 deploy.sh boot`).
Rollback: generation **31** = `/nix/store/na3yx1c2yqyaf5vi1lk7n915aylwp1a1-nixos-system-as7772-26.11.20261111.d4f25a3` (47c0012 SAI + Rust kernel + Rust fan driver), still in the profile list.
libsai `pvg2c84d...-sai-openbcm-rs/lib/libsai.so.1.0.0`. Box clock is PST in transcripts; times here UTC. No PHY hang, no
power cycle. Lab CAK grepped in every file here: 0 hits. Helpers used on the switch (not in the repo, text embedded in
c3 / f1 / f2): `/root/h5-with-pairs.sh`, `/root/h5-lat.sh`, `/root/h5-macsec-off.sh`.

| file | UTC | what | result |
| --- | --- | --- | --- |
| a0-baseline.txt | 2026-11-23T23:02:16Z | baseline: system, fan driver, failed units, MACsec, CONFIG_DB | gen 31 na3yx1c2 booted; as7760_fan bound (hwmon7), fand active, failsafe false; 0 failed; 8 SAs; CONFIG_DB/kernel already clean (no VRFs) |
| b1-cleanup.txt | 2026-11-23T23:04:34Z | step 1: script copy, cleanup dry-run, cleanup, ring check | scripts from 3c84eb3 copied (sha256 equal); --cleanup --dry-run: no writes needed; --cleanup: RESTORED and verified (142 keys); ring check OK. `bash -l script` returns 1 on every exit (bash_logout + set -u), inner rc 0 |
| b2-closure-diff.txt | 2026-11-23T23:05:37Z | step 2: closure diff gen 31 -> 3c84eb3 | only sai-openbcm-rs (+206.6 KiB) and derivations that embed its path; kernel, modules, initrd, fan module, fand identical; ext-phy/ext-macsec tables byte-identical |
| b3-deploy-boot.txt | 2026-11-23T23:05:52Z | DEPLOY_REV=3c84eb3 deploy.sh boot | rc 0; system-32 = azkkiphr default |
| b4-reboot-postboot.txt | 2026-11-23T23:09:11Z | reboot into gen 32: system, fan driver, failed units, PHYs, libsai maps, up-check | azkkiphr booted; fan bound, no failsafe; 0 failed; 32/32 PHYs in 102.9 s; libsai pvg2c84d; up-check PASS 14.7 s |
| c1-sonic-traffic-test.txt | 2026-11-23T23:12:58Z | sonic-traffic-test.sh (new) | 0 lost; RESTORED and verified; rc 0 |
| c2-macsec-traffic-test.txt | 2026-11-23T23:12:58Z | macsec-traffic-test.sh (new, restores CONFIG_DB) | 0 lost; encrypted = accepted (3 SCs); 0 ingress errors; RESTORED and verified; rc 0 |
| c3-macsec-rekey-test.txt | 2026-11-23T23:21:23Z | macsec-rekey-test.sh 20 s / 100 s inside the VRF-pair wrapper | (VRF-pair wrapper) 100000+100000+10000 pings 0 lost; 68 egress SA installs; restore verified; rc 0 |
| c4-macsec-wrongcak-test.txt | 2026-11-23T23:23:01Z | macsec-wrongcak-test.sh inside the VRF-pair wrapper | (wrapper) wrong CAK: cage 1-2 100 % loss, 31-32 0 %; restored 0 %; MKA 6 s; rc 0 |
| c5-phy-status-extphy.txt | 2026-11-23T23:23:01Z | phy-status (bare reads) + openbcm-diag extphy new counter lines | 32 of 32; new extphy counter lines present |
| d1-phystat-single.jsonl | 2026-11-23T23:23:58Z | phystat JSON lines | raw reads (temp_c 410.0 = raw 0, pre-existing on the diag backend) |
| d1-phystat-single.txt | 2026-11-23T23:23:58Z | phystat.py single run, txn phy | rc 0, 12.6 s; 0 stopped / read 0x0000 / FW failure / poll fault; txn 228 begun = ended, 0 expired |
| d2-phystat-x5.jsonl | 2026-11-23T23:26:02Z | phystat JSON lines | raw reads |
| d2-phystat-x5.txt | 2026-11-23T23:26:02Z | phystat.py x5 back to back | 5 x rc 0; 0 stopped (G6: 13); txn 1368 = 1368, 0 expired; forced reads 12 -> 22, max wait 53 ms |
| d3-phystat-txn-off.jsonl | 2026-11-23T23:26:02Z | phystat JSON lines | raw reads |
| d3-phy-status-and-txn-off.txt | 2026-11-23T23:26:02Z | phy-status (bare reads) + phystat --txn off | phy-status rc 0, 32 implicit-lease NOTICEs; --txn off rc 0; 0 stopped in both |
| d4a-rekey-phystat-loop-attempt1.txt | 2026-11-23T23:31:40Z | attempt 1: phystat --interval 0 ran once only (invalid overlap) | invalid: --interval 0 = one run only |
| d4-rekey-phystat-loop.jsonl | 2026-11-23T23:38:19Z | phystat JSON lines | raw reads of the 17 good runs |
| d4-rekey-sa-counter-samples.txt | 2026-11-23T23:38:19Z | raw samples | COUNTERS_DB SA counters every 2 s |
| d4-rekey-with-phystat-loop.txt | 2026-11-23T23:38:19Z | rekey 20 s/40 s concurrent with 19 phystat runs | 19 phystat runs during rekey 20 s/40 s: 17 ok, 2 failed on an expired lease; rekey 0 lost, 28 installs; 2 SA sequences waited 1001/1007 ms (lease expiry); restore verified |
| d5-macsec-traffic-after-tools.txt | 2026-11-23T23:40:05Z | macsec-traffic-test after the tool sessions (SONiC MACsec counters advance, equal) | after the tool sessions: equal, 0 errors, rc 0 |
| e0-ring-pre.txt | 2026-11-24T00:17:56Z | ring -10..-2: probe, checks, p0, p1 | probe clean; ring check OK; up-check FAIL only on the restart criterion (4 ports secured) |
| e1-ring-up-inject.txt | 2026-11-24T00:17:56Z | sonic-ring.sh up + inject, sampler unit | up rc 0; 740 + 740 injected |
| e2-ring-samples.txt | 2026-11-24T00:17:56Z | sonic-ring.sh sample every 60 s (29 samples) | 29 samples; 92.07 Gb/s per port; 8 SAs, 0 failed, no failsafe throughout |
| e3-ring-macsec.txt | 2026-11-24T00:17:56Z | sonic-ring.sh macsec every 5 min | 6 MACsec snapshots |
| e4-ring-phystat.jsonl | 2026-11-24T00:17:56Z | all ring phystat reads (probe, p0, p1, 6 during, after), labelled | 10 reads x 4 PHYs: FEC corr / uncorr / symbol all 0 |
| e5-ring-phystat-journal.txt | 2026-11-24T00:17:56Z | syncd journal per phystat read | every read: 0 stopped / FW failure / Failed to get attr / poll fault / expired |
| e6-ring-stop-down.txt | 2026-11-24T00:17:56Z | sonic-ring.sh stop + down | down: RESTORED and verified |
| e7-ring-post.txt | 2026-11-24T00:17:56Z | post-ring: counters, up-check, traffic test, check | traffic test equal, rc 0; ring check OK |
| e8-ring-conservation.txt | 2026-11-24T00:17:56Z | computed ring balances | cables: TX = peer RX exactly (frames, bytes); drain 1480 = 1480; MACsec encrypted = accepted x4; PHY line MIB equal (1 of 4 off by 2: read skew) |
| f1-latency-arm-on.txt | 2026-11-24T00:26:09Z | link-down latency arm ON, 20 cycles | 20 cycles; next-pass 8/20; total min 41 / med 200 / mean 223 / max 477 ms |
| f2-macsec-off-method.txt | 2026-11-24T00:26:09Z | MACsec-off method (Upholds), clear-text traffic check, restore | systemctl stop does not stop macsecmgrd (orchagent Upholds=); MACsec off via CONFIG_DB PORT.macsec; clear-text sonic-traffic-test 0 lost; restored, CONFIG_DB identical |
| f3-latency-arm-off.txt | 2026-11-24T00:26:09Z | link-down latency arm OFF, 20 cycles | 20 cycles; next-pass 10/20; total min 12 / med 260 / mean 263 / max 513 ms |
| g1-final-reboot.txt | 2026-11-24T00:31:33Z | final reboot: system, fan, PHYs, failed unit + its fix | azkkiphr booted; fan bound; 32/32 101.5 s; sonic-nix-apply failed at boot (tmp-file race), restarted: 0 fields written; 0 failed |
| g2-final-macsec-up-check.txt | 2026-11-24T00:31:33Z | final macsec-up-check | PASS 15.7 s |
| g3-final-macsec-traffic-test.txt | 2026-11-24T00:31:33Z | final macsec-traffic-test | equal, 0 errors, RESTORED and verified, rc 0 |
| g4-final-ring-check.txt | 2026-11-24T00:31:33Z | final sonic-ring.sh check | CHECK OK |

## Ring FEC (corrected / uncorrected / symbol errors summed over 4 lanes; clear on read, so per interval)
| read | phy 0x00 | phy 0x01 | phy 0x17e | phy 0x17f | rc | s |
| --- | --- | --- | --- | --- | --- | --- |
| probe | 0/0/0 | 0/0/0 | 0/0/0 | 0/0/0 | 0 | 12.6 |
| p0 | 0/0/0 | 0/0/0 | 0/0/0 | 0/0/0 | 0 | 12.6 |
| p1 (idle 60 s) | 0/0/0 | 0/0/0 | 0/0/0 | 0/0/0 | 0 | 12.5 |
| t+5 min | 0/0/0 | 0/0/0 | 0/0/0 | 0/0/0 | 0 | 12.8 |
| t+10.61 min | 0/0/0 | 0/0/0 | 0/0/0 | 0/0/0 | 0 | 12.7 |
| t+15.05 min | 0/0/0 | 0/0/0 | 0/0/0 | 0/0/0 | 0 | 12.6 |
| t+20.02 min | 0/0/0 | 0/0/0 | 0/0/0 | 0/0/0 | 0 | 12.5 |
| t+25.01 min | 0/0/0 | 0/0/0 | 0/0/0 | 0/0/0 | 0 | 12.6 |
| t+30.07 min | 0/0/0 | 0/0/0 | 0/0/0 | 0/0/0 | 0 | 12.9 |
| after | 0/0/0 | 0/0/0 | 0/0/0 | 0/0/0 | 0 | 12.5 |

ASIC 45.7-48.1 C (rust-g4-load peak 48.6 C), no failsafe; syncd 10.6-11.9 % CPU, RSS 139580 -> 139668 kB.
`temp_c` reads 410.0 (raw 0) on every read: pre-existing on the diag backend (rust-g4-load and rust-g6 h2 show the same).

## Link-down latency (ms; t0 = syncd "admin down" of the far port; X3 section 4 method, no diag socket in the arms)
| arm | n | next pass | total min / median / mean ± SE / max | mean same / next pass | R | G median | hset -> t0 | from far Lost carrier (mean) | up min / median / max |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| MACsec on | 20 | 8 (40%) | 41.4 / 200 / 223 ± 33.4 / 476.9 | 117.3 / 381.7 | -3.2 .. 1.3 | 3.1 (max 5.9) | 36.1 | 235.1 | 580 / 823 / 1061.7 |
| MACsec off | 20 | 10 (50%) | 11.5 / 259.8 / 263.4 ± 35.9 / 512.7 | 124.1 / 402.7 | -15.6 .. 2.1 | 3.1 (max 18.7) | 36.2 | 275 | 609.9 / 838 / 1064.2 |

P (pass period, from the next-pass runs) 261.8 / 260.9 ms.

## Findings
- MDIO arbitration: single and x5 phystat runs, phy-status and --txn off: 0 "stopped" / "read 0x0000" / FW failure / poll
  fault lines (G6 had 13 in x5); transactions begun = ended, 0 expired.
- Under a rekey with a back-to-back phystat loop: 2 of 19 runs failed on an expired lease. Each time an SA sequence on
  the leased PHY waited the whole 1 s lease (1001 / 1007 ms) and the client's next request reached syncd only right
  after that. The diag accept loop serves one connection at a time (sai-diag server.rs accept_loop -> serve), so a
  locked command queued in front of it (the rekey test polls `openbcm-diag extmacsec` every 1 s while MKA comes
  back) waits for the API lock, while the SA install holding that lock waits for the lease: the lease expiry breaks
  it. The timing is verified; that the queued connection was the test's extmacsec poll is inferred. 0 pings lost.
- "waits for a library run before a poll" max is 72-89 ms (also right after a boot, with no tool running), not the
  ~20 ms X1 expected: the gate waits for SA-install runs (extmacsec max_us 80-90 ms), which holds up the link-scan pass.
- Latency: H0 (poll phase) is supported: 40 % / 50 % next pass, R within -3.2..+2.1 ms except one OFF cycle
  (-15.6 ms, G 18.7 ms, with MACsec off). H-reg is rejected (it predicts >= 90 % next pass). H-lock gets no support
  (ON - OFF mean = -40 ms, SE about 49). H-t0: the hset return adds a constant 35-46 ms.
- `systemctl stop sonic-macsecmgrd` does not turn MACsec off: orchagent's Upholds= restarts it within 10 ms (f2).
- `bash -l script.sh` exits 1 whenever a set -u script calls exit (NixOS /etc/bash_logout reads an unset variable):
  judge the scripts by their own rc.
- sonic-nix-apply.service failed once at boot (FileNotFoundError on the fixed /run/sonic-nix/issue.tmp, sonic-nix-watch
  started the same second); restarted, 0 fields written.

```
1965aec54f58c942cf759e802a2085bb6e2b802b93d5f3abc2d86d200a674d2f  a0-baseline.txt
59cf6c4e41a690ed9adcefad1a78fc6c2738f06b16bacfda9659ccf8706a7b3f  b1-cleanup.txt
8b4c748fc3b0c7823cb40a7eb7803c81125b0f446c3dc93c396ea94017eebe93  b2-closure-diff.txt
3833f2c60664553cf57b33792ef696162c445f376460f960ab11feaf2cef6ea4  b3-deploy-boot.txt
86a44efcb16caa66e430e2f9bb90be80d3185de8cac748f322afe8650c45cd21  b4-reboot-postboot.txt
91b56bc2f92c67ade59b209fcf599a0a6fd19c4946c6c5be6c5fa544629598db  c1-sonic-traffic-test.txt
6e313259732398d398500d0276ddb7eb82d3441875073e5506683917e185689e  c2-macsec-traffic-test.txt
18540f87294cf491e6c5f5cc4a36363488f0085320f3f922892912310d6fd2b9  c3-macsec-rekey-test.txt
eff0c24009bb7336ff5242d189233cac77fbf186c0908784a1afc608ff8c3844  c4-macsec-wrongcak-test.txt
f76f36f69c513a939cc300651be3910747b8e48bdabe5674c885f1aaa3aad30c  c5-phy-status-extphy.txt
fbb3df3a802fff88b37d44a1e7bdfd1a94fa79dfadb471f9140b977e7898d276  d1-phystat-single.jsonl
da3af3d0117fb5adef2df24a9259330b273ea4db95750a920036cc2ff98e9523  d1-phystat-single.txt
7dc87cad2c05475837b569084f7477c2c8d467a5da416056f732ba3a22d0a657  d2-phystat-x5.jsonl
52578ae52e0703e042416147174c141622d48f0a12d41f6b83f19363ec60d6cf  d2-phystat-x5.txt
f7ad6931abd6c4bae4d546cf30e436f51a924c4b96cd9c75ec5c2b78701cefb6  d3-phystat-txn-off.jsonl
e76deef952a0342c8012a585c9e1ed0044c799a1143bfd83bbd46d4cbf3f51f4  d3-phy-status-and-txn-off.txt
384427364c09c5725210094a699dafb4f9ef4290754d187fb31892f3ffb1c2df  d4a-rekey-phystat-loop-attempt1.txt
c9c27ae31eeab63aed8108c89961ba0749e2877b0cb28692dcd6d0ead221ffb4  d4-rekey-phystat-loop.jsonl
3bec679290e66e11f098c0ef56432d8111e32c140b10c2c8c71ab5d34b4b32d3  d4-rekey-sa-counter-samples.txt
8cb1d34430bcb7f86536d24f8c5666afbd0f646ac0a6293c127445a53d41c1ef  d4-rekey-with-phystat-loop.txt
d36cded1574bc3bb1b578b89691706b47895128f42baa8e6e7e08aad09a956c7  d5-macsec-traffic-after-tools.txt
241507095a9e80e07f76e21ede1e06d89c573444edef1405e054d4144bd4c379  e0-ring-pre.txt
d767806cebe4f7dc98c52367b224957916cf724ffaa9f13facdc88ef49e3ed70  e1-ring-up-inject.txt
06c24490e1224cd8c11e36b747c6a0441ef31128579f0ac785be28cffd26b052  e2-ring-samples.txt
27ee0faff8dd9869aade6077718cfa625a2836d79b833e00149df1d6eac85074  e3-ring-macsec.txt
50791cc980f0ec5d55697656310d5b2dca79112d1f136716d50b2f995a3eb6ae  e4-ring-phystat.jsonl
49f66c917c76139ec0b1db7a5c6027533cf0e21c9c8c1da0b52c2493110a269a  e5-ring-phystat-journal.txt
4410540fc23415adbf6f4f5fd65c1bc8b9247652719b8cc0aa5cdbcbee1131d9  e6-ring-stop-down.txt
4c3621d6b88b6f988a0d2aaf8600f55af434631058575aa5cd38384a0fe3fb1e  e7-ring-post.txt
1c99b2e2f67f595959758d04fee0324c20573036b7e42164c4f6efb86d06c464  e8-ring-conservation.txt
7cba6dbc6457e665449cf25c6584b6c517bd9f40bb1f3085c2124ccdd86c66c1  f1-latency-arm-on.txt
ec8cbe8e313fc5e5f62762c55fda09a6d19bac7324089e2eb191ff7700a11a9e  f2-macsec-off-method.txt
0ac8cb587c31e80bbd7143cbfb242e148d0e3b437d9c5fbad7c3a1fc50feb472  f3-latency-arm-off.txt
c2c807e4b5badcce82b6231ae6a065cb6493ea914dfbb4bf5931bf3a170fe1a9  g1-final-reboot.txt
4ab9bf7bfc76b5706403872b2ceda40af3dc4aeea6afa876d5aaa1a1b14973d8  g2-final-macsec-up-check.txt
adb034eb8a8dc12d1085bd1314db2ccd09f046b21d1ec44226386a17509f68e7  g3-final-macsec-traffic-test.txt
77a811a0305e66b86f1d994477a01b7c4dd3c56d55fd91b5486977ec566c34c5  g4-final-ring-check.txt
```

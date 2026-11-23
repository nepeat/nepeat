# AS7772 G6 hardware check of committed 47c0012 (Rust the only SAI) + agent F's MACsec-poll hold-off (agent H4, 2026-11-23 UTC)

Boot default and final system: **47c0012 #as7772** = `/nix/store/631c5hx7kznld417zvpvp38w7ml2dc05-nixos-system-as7772-26.11.20261111.d4f25a3`
(`nix eval` of git rev 47c0012 toplevel gives the same path; before: 64296gcv = 50a6aa7 as7772-rust). syncd maps
`b8qllq61...-sai-openbcm-rs/lib/libsai.so.1.0.0`; openbcm-mdio-script runs from the same package. Working-tree test
system (agent F's fix, `deploy.sh test`, never the boot default): `kh01796ssvawwww60a9q8h1sxlz9bnls`, libsai `5l1l61fp...-sai-openbcm-rs`.
Box clock is PST in transcripts; times here UTC. One clean cold power cycle (planned, e2); no PHY hang. Lab CAK grepped
in every file here: 0 hits. Wrap-test keys: fresh, `dumps/rust-g6-wrap/` (git-ignored), deleted on the switch after.

| file | UTC | what | result |
| --- | --- | --- | --- |
| a0-soak-20261123.txt | 13:31-16:05Z | soak monitor of 64296gcv (32 samples, 5 min) | links 4/4 up, 8 SAs, 0 failed units, 0 port errors, 0 journal err after the boot sample; syncd pid unchanged, RSS 138640 -> 138928 kB; ASIC 45.7-48.1 C |
| b0-deploy-boot.txt | 16:09Z | DEPLOY_REV=47c0012 deploy.sh boot | rc 0, 631c5hx7 default |
| b1-reboot-state-mdio.txt | 16:12Z | reboot: system, MDIO tool, syncd maps | 32/32, 0 differed, 102.2 s; Rust libsai |
| c1..c3 | 16:13-16:15Z | macsec-up-check, sonic-traffic-test, macsec-traffic-test | PASS 15.8 s; 0 lost; encrypted = accepted, 0 ingress errors |
| c4-macsec-rekey-test.txt | 16:15Z | rekey 20 s / 100 s (standard) | 210000 pings 0 lost, 68 egress SA installs, MKA up after 7 s |
| c5-macsec-wrongcak-test.txt | 16:23Z | wrong CAK on cage 1-2 | 100 % loss on 1-2 (ICV mismatch), 31-32 0 lost; restored 0 lost |
| c6-macsec-wrap-test.txt | 16:24Z | static XPN-256, sets low + wrap (pn 2^32-300) | 600/600 both sets, 0 failed units |
| c7-phy-status.txt | 16:25Z | phy-status + openbcm-diag extphy | 4 DAC ports up, 0 faults |
| c8-led-readback.txt | 16:25Z | LED data RAM read-back (leds.md method) | link bytes 1 at exactly LEDUP0 0xa8/0xac, LEDUP1 0xb0/0xb4; frame bytes = leds.md; LEDUP_EN=1 both |
| c9, c10 | 16:25-16:27Z | far end (Ethernet4) admin down / up x3 | Ethernet0 oper down 491 / 305 / 282 ms after syncd "admin down" (documented 67-141 ms: **slower**); LED 0xb0 follows; back up 0.76 s; MKA PASS |
| d1-smoke-tables-and-recreate.txt | 16:27Z | sai-openbcm-smoke, SONiC stopped, board tables; --recreate | 22 PASS 1 SKIP 0 FAIL; 23 PASS 0 FAIL; knet lines 0 |
| d2-after-smoke-macsec-up-check.txt | 16:29Z | sonic.target started again | PASS 16.1 s |
| e1-warm-* | 16:31Z | reboot | 32/32 102.0 s; PASS 15.3 s; 0 lost, equal |
| e2-cold-* | 16:33-16:38Z | poweroff, lab-power 1 off 20 s, on | 32/32 102.9 s; PASS 15.1 s; 0 lost, equal |
| f1-phystat-clash-repro.txt | 16:38Z | phystat.py once on 631c5hx7, MACsec up | **reproduced**: 1 FW Message failure, 6 syncd "pn.* stopped ... 1.8221 read 0x0000" lines (ce0, ce1); SAs stayed 4/4 |
| g0-deploy-test-worktree.txt | 16:39-16:45Z | deploy.sh test, working tree | rc 0, kh01796s active (syncd restarted) |
| g1-worktree-up-check-maps.txt | 16:45Z | maps + up-check | libsai 5l1l61fp; PASS 16.6 s |
| g2-phystat-x5-worktree.txt | 16:45-16:46Z | phystat.py x5 back to back | tool: 0 failures in 5 runs; syncd "stopped" lines 2/3/7/0/1 (13); "deferred" NOTICE and "deferred 10040 ms, the most allowed" lines present |
| h0..h4 | 16:46-16:47Z | sonic-ring.sh check/up/inject/stop/down on kh01796s | check OK; **up exit 1** (INTERFACE Ethernet4/124 still carried macsec-traffic-test's vrf_name), no ring traffic; aborted, down |
| z0, z1-final-* | 16:48-16:51Z | final reboot into the boot default | 631c5hx7 booted, 32/32 101.9 s, 0 failed, PASS 15.7 s, 0 lost, equal; VLAN/FDB keys none; no soak timer |

## Findings
- phystat clash on 47c0012: reproduced (f1). With F's hold-off (g2) the tool no longer fails, but syncd still logs
  "stopped" lines when the runs are back to back: 5 x ~8 s with < 3 s between keeps the hold-off up past the 10 s
  bound, and the forced polls then collide (F-READY item 3 predicts this; not separated from other causes here).
  Single runs with a gap were not tried. Fix **partly** verified.
- Link-follows-cable: correct, but 282-491 ms instead of 67-141 ms. MACsec was on here (the documented runs were
  not with MACsec under SONiC, per sonic.md); not explained.
- FEC under load: **not measured**. h2 (before) only, idle.

```
bdb8379d0b35ecf7e5af7e0aac352db23e99a40de68ad3bad5d18985623368c6  a0-soak-20261123.txt
d5a8e3592e97c480910caa628b698e2698e59a18404edccdbcb66312e066bf98  b0-deploy-boot.txt
9a9466ce84ca832eda6f98baa29ae2000603c33c023f60a40a6e84d96b046a4c  b1-reboot-state-mdio.txt
433c89a8bdda9ad8ff133b6de195ae4886417d67b7e093209fbe43a2b1d9d134  c10-link-follows-repeat.txt
b04c6c035c3c8ddccdebd155d703bfcf8df962a872632969034c73e836108a41  c1-macsec-up-check.txt
626ae51cf0d428090dcc17cef688276ea396e9935772d93bee72b56e9351f0c0  c2-sonic-traffic-test.txt
66e80c6ec597a3a646cac3f744ab6252b961553f5a98d700a3c03d551b4d00f2  c3-macsec-traffic-test.txt
4adee013efb483c0aaa2cbe1729c75f6c49b2a66217496fbd3c883c50e1bb0c7  c4-macsec-rekey-test.txt
d1e02dc0928f0fcc94f3c8a51fee6eab156ae5300bbc6f56dae0b230ff4fed8e  c5-macsec-wrongcak-test.txt
2721d5789b349bd0aaf9e446f75c00ed80967c61a0e5c76830c3b879a90f9676  c6-macsec-wrap-test.txt
838d400bfc1bdb322241925c852d12737a54516e7db1f0adad9f57b88867a55f  c7-phy-status.txt
bf6be8e3c1a68b438a4bbe9746b17817a8e15ece7e5de2826613d77cd8dd8f71  c8-led-readback.txt
1e61c19d911c1e051786783326055b35098623708be36be7e7f807c7bf620d13  c9-link-follows-cable.txt
16f4eceeb8ef6f91b53d5c48be6cd83916443b490b7454ef5b303d160f7f0ee2  d1-smoke-tables-and-recreate.txt
9b628904a69f92c6b342a7cc6e84c3341c13f198a787332b8218a3419014ed26  d2-after-smoke-macsec-up-check.txt
5f421d0e4d1cb356a56a8fef9fb7de3b2ea4017b20a3de2b34d35abc9abb89c7  e1-warm-macsec-traffic-test.txt
bc7d544b91f95c0f6edb3548cce04605265f3dbef66f66cd25fbf9e5027e9f32  e1-warm-macsec-up-check.txt
6406f2d813a328e6e67ac7fea6d29eef9e86d03c2ea5d1de0dcf9a72f1da5bc1  e1-warm-state.txt
2349f3bf9592b1330a39b68323770626d155ceed5fd2f0b155fb1bb070dbc545  e2-cold-macsec-traffic-test.txt
dd372dec737aff1274547832c1c89163b160224805f2da798cfc63fd848950da  e2-cold-macsec-up-check.txt
a9cebe98f7a97d62caf24f5f61da66cdc9024657ef0538f424120053dfca73e6  e2-cold-power.txt
ad9ad101001a8ca760fec659a5064dfffdea16e25855bec0eea710d934c2bb7a  e2-cold-state.txt
1be4e6960c5cc9a028d14d333742f9669d2cb8878d84cff040d22f48cf9bfd5d  f1-phystat-clash-repro.txt
353e29610f77dca8ef3caedeb1bda222202b4bc8709ed62a319ef54b4d771f0f  g0-deploy-test-worktree.txt
71f2d4ceecfc541dab6006a7752416098cd9a523f3a735a6f7ba8e5e10d4f525  g1-worktree-up-check-maps.txt
7f1373d6ed74f26625636d8aac0adb3d21cd5d442d814372858aa7d67d7cf344  g2-phystat-x5-worktree.txt
db877e4c991cb4840584488815af2cdf5522f9d0ce67ceaed74363d094d7d553  h0-ring-check.txt
7002fffefcae39ccbd9dadb61e18d2c12a8e97e49983d2325c0ca243600fdd6e  h1-ring-up.txt
c55dbe12f282215cecff34cb73355e7c41ac2b6b449ccf0f669cb2a8f2b83d01  h2-phystat-before.txt
eda7c2705acbfd8d276af3f53f6b9855279d7448c229982576142df73a0209c2  h3-ring-inject.txt
ca9125746712545019b463c54288a521ed7d811d32d6269ed7a127f06d015215  h4-ring-stop-down-aborted.txt
6f66ecb8bda6365d356f1edcf0890e1d55d2d5d31841bdfc45f9568cf200dd37  z0-final-ring-gone-soak.txt
17d3e8a1da9a5ef68a8db2adfcf061fc2c7e57aaaa2134443242b33142d54800  z1-final-macsec-traffic-test.txt
6cc2e2318c5e76b8af22b4c41cc8bf3d92c5d10acca8e4c514252b5021fe43f9  z1-final-macsec-up-check.txt
18e0cee0fb563c0410e26e8f2f8efbf106b6554968fe76a3ccb46a867b107c75  z1-final-state.txt
```

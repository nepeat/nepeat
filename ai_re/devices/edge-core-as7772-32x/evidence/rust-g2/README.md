# AS7772 G2 smoke, G3 part 1, G4 trial for the Rust SAI (agent H2, 2026-11-23 UTC)

Working tree (uncommitted G2 work, HEAD `dfa6185`), deployed as git flake. `git add -N` by H2:
`rust/sai/crates/sai-macsec/tests/common/seeded.rs`, `rust/sai/crates/sai-macsec/tests/cpp/listed_fixes.rs` (sai-macsec's
build.rs includes the latter). Box time zone is PST; times here are UTC.

Systems: **before** `q4nbqvf2a7hc...` (C++). **C** `z7jccivx51iml5sqri0qanipv46gs1ll` (attr as7772: C++ libsai
`7gqia5n3...` with the sdk_glue range checks, C MDIO tool). **rust-mdio** `2kk34h17krxi21j7pn2yzsanpxmphjw4` (C++ libsai
`7gqia5n3`, boot unit = Rust tool `13y9svn0...openbcm-mdio-script-rs`). **rust** `zvih1crw43xfr1nvs33gqmr9l7dj6nm2` (attr
as7772-rust: libsai, diag, asic-temp, MDIO tool all from `j0jjhc94...-sai-openbcm-rs`). The smoke ran `lxisf5bv...-sai-openbcm-rs`
(from the as7772-rust build `4kbg16j6...` 35 min earlier); `j0jjhc94` differs from it only in
`share/sai-rs/libsai-openbcm.txt` (a RUNPATH line): every binary is byte-identical (the tree changed under M2's test edits
between the two builds). **End state: rust `zvih1crw` running and booted** (D passed). No PHY hang; power cycles: 3 planned
cold boots (10:57Z, 11:04Z, 11:39Z: plug 1 off 20 s). Lab CAK grepped on the switch in every file here: 0 hits.

| file | UTC | what | result | sha256 (16) |
| --- | --- | --- | --- | --- |
| a0-baseline-q4nbqvf2-syncd-warnings.txt | 10:14Z | before: q4nbqvf2 (C++), syncd/orchagent warnings, normalised | reference set | 13125a44ac892945 |
| a1-deploy-switch.txt | 10:15Z | A: deploy.sh switch (attr as7772, working tree) -> z7jccivx | rc 0, syncd restarted | 04839342b7eaef02 |
| a2-C-reboot-macsec-up-check.txt | 10:18Z | A: C after one reboot (C tool bring-up 100.9 s, 0 differ) | PASS 15.7 s | 72eaab84beea73d1 |
| a2-C-reboot-sonic-traffic-test.txt | 10:19Z | A: C | 6400 pings 0 lost | ea5329b9466c1641 |
| a2-C-reboot-macsec-traffic-test.txt | 10:21Z | A: C | 0 lost, encrypted = accepted | 17f8d1402e475ac9 |
| a2-C-reboot-macsec-rekey-test.txt | 10:29Z | A: C, rekey 20 s | 210,000 pings 0 lost, 68 egress SA installs | bb1c5f8420605dcb |
| a3-C-syncd-warnings.txt | 10:29Z | A: C warnings vs a0 | same set (range checks reject nothing) | e8811093ea245a99 |
| a4-C-tool-status.json | 10:29Z | A: C MDIO tool status.json of that boot | reference for C | 72a838b4e2c0af5e |
| b0-cxx-reference-ps-knet-temp.txt | 10:37Z | B: C++ reference: diag ps, extphy, KNET, show temp | reference | fa830c31701a41fe |
| b1-smoke-noextphy.txt | 10:45Z | B run 1: sai-openbcm-smoke, Rust libsai lxisf5bv, --no-ext-phy | 21 PASS, 1 FAIL (temperature: smoke-check race), 1 SKIP | ccf9aa57eeb94e1c |
| b2-smoke-extphy.txt | 10:46Z | B run 2: with the board ext-PHY/MACsec tables | 21 PASS, 1 FAIL (same race, other direction), 1 SKIP; oper up 1038 ms | 04afec1204ef4609 |
| b3-smoke-window-journal.txt | 10:45-10:46Z | B: journal of both runs (fand: smoke publisher readings) | temperature published by the Rust lib | 1bc96edf6774faaf |
| b4-C-after-smoke-macsec-up-check.txt | 10:48Z | B: sonic.target started again (C++), no reboot | PASS 15.8 s | e34c7df1529c2fa8 |
| b4-C-after-smoke-sonic-traffic-test.txt | 10:49Z | B: C++ after smoke | 0 lost | f4a772b5b99cf8cd |
| b4-C-after-smoke-macsec-traffic-test.txt | 10:50Z | B: C++ after smoke | 0 lost, equal | 0abce6fcdd49aaf0 |
| b5-cxx-temp-jitter.txt | 10:50Z | B: six consecutive show temp (C++): monitor 5 46.2-47.2 C | explains the temperature FAIL | de0635f2d89c0798 |
| c0-deploy-rust-mdio-boot.txt | 10:50Z | C: DEPLOY_ATTR=as7772-rust-mdio deploy.sh boot -> 2kk34h17 | rc 0 | 51354e110cf06a0f |
| c1-rustmdio-reboot-progress.txt | 10:51Z | C: reboot, bring-up progress | Rust tool ok, 97.8 s / took 101.3 s | c374900db429ffe2 |
| c1-rustmdio-reboot-mdio-status.txt | 10:53Z | C: status.txt, unit status, phy-status | 32/32, 0 differed | a217110c9c879346 |
| c1-rustmdio-reboot-tool-status.json | 10:53Z | C: Rust tool status.json | = C except poll re-reads + summary seconds | b36ff97d72ee0678 |
| c1-rustmdio-reboot-macsec-up-check.txt | 10:54Z | C: reboot | PASS 14.8 s | e9bfac410409a8d3 |
| c1-rustmdio-reboot-sonic-traffic-test.txt | 10:54Z | C: reboot | 0 lost | e17bbe34467c9131 |
| c1-rustmdio-reboot-macsec-traffic-test.txt | 10:55Z | C: reboot | 0 lost, equal | 60cfd766855b07a8 |
| c2-rustmdio-cold1-power.txt | 10:56-10:59Z | C: cold boot 1 (poweroff, plug off 10:56:58Z, on 10:57:18Z) | Rust tool ok 97.7 s / 101.3 s | 152543bfc7e8b554 |
| c2-rustmdio-cold1-mdio-status.txt | 11:00Z | C: cold 1 | 32/32, 0 differed | 5a40847f050c3a25 |
| c2-rustmdio-cold1-tool-status.json | 11:00Z | C: cold 1 | = C except re-reads + summary seconds | a3c0c602a2ffe196 |
| c2-rustmdio-cold1-macsec-up-check.txt | 11:00Z | C: cold 1 | PASS 15.5 s | efc0721a31810af0 |
| c2-rustmdio-cold1-sonic-traffic-test.txt | 11:01Z | C: cold 1 | 0 lost | a6abeee0fe3657d3 |
| c2-rustmdio-cold1-macsec-traffic-test.txt | 11:02Z | C: cold 1 | 0 lost, equal | d049def7619070cf |
| c3-rustmdio-cold2-power.txt | 11:03-11:06Z | C: cold boot 2 (plug off 11:03:52Z, on 11:04:13Z) | Rust tool ok 97.8 s / 100.5 s | 80d061db204632bd |
| c3-rustmdio-cold2-mdio-status.txt | 11:06Z | C: cold 2 | 32/32, 0 differed | dfd9a03e2772a722 |
| c3-rustmdio-cold2-tool-status.json | 11:06Z | C: cold 2 | = C except re-reads + summary seconds | bcd2701dbcf5ae42 |
| c3-rustmdio-cold2-macsec-up-check.txt | 11:06Z | C: cold 2 | PASS 15.8 s | 385fbc7640914fc7 |
| c3-rustmdio-cold2-sonic-traffic-test.txt | 11:07Z | C: cold 2 | 0 lost | 951e019571fb68de |
| c3-rustmdio-cold2-macsec-traffic-test.txt | 11:08Z | C: cold 2 | 0 lost, equal | b6c39b950b63f89e |
| d-trace-cxx-g2.txt | 11:10Z | D: C++ reference trace (libsai 7gqia5n3 on 2kk34h17), keys **** | 237 runs, 0 failed, 0 dropped | f75767c2ea546a5a |
| d0-deploy-rust-switch.txt | 11:17Z | D: DEPLOY_ATTR=as7772-rust deploy.sh switch -> zvih1crw | rc 0, syncd on Rust libsai j0jjhc94 | f07d0811983a3798 |
| d1-rust-syncd-warnings.txt | 11:18Z | D: warnings incl. the C++ session of the boot | no new warning type | 243074ada6a2653d |
| d2-syncd-journal-cxx-vs-rust.txt | 11:19Z | D: normalised syncd journal, C++ session vs Rust session | differences listed (see notes) | 747a3a76416e5e95 |
| d-syncd-journal-boot.txt | 11:19Z | D: syncd journal of that boot (C++ then Rust) | for the developers | 461fadde8b29dd70 |
| d-rust-session-journal.txt | 11:19Z | D: syncd+orchagent+swss+portsyncd+macsecmgrd since the Rust syncd start | for the developers | f2dcdc494fa40336 |
| d3-rust-state.txt | 11:19Z | D: ports, KNET, LLDP, temps, fand, extphy, extmacsec, ps, phy-status | 32 ports, 4 up, 32 netdevs, 4 LLDP, 32/32 PHYs | df1e7b722a29140f |
| d4-rust-macsec-up-check.txt | 11:20Z | D: Rust | PASS 16.5 s | 57aa3b8bdeb08d52 |
| d4-rust-sonic-traffic-test.txt | 11:20Z | D: Rust | 0 lost | f99705a09a72c260 |
| d4-rust-macsec-traffic-test.txt | 11:21Z | D: Rust | 0 lost, equal | 7583638ca11936b5 |
| d5-rust-macsec-rekey-test.txt | 11:29Z | D: Rust rekey | 210,000 pings 0 lost, 68 egress SA installs | 5ca0e493ee674c79 |
| d5-rust-macsec-wrongcak-test.txt | 11:30Z | D: Rust wrong CAK | no SAs, 0/100 (+90 ping errors), other link 100/100, back in 6 s | 33dad1d31622e3fe |
| d-trace-rust-g4.txt | 11:31Z | D: Rust trace 1 (started from AN 1 after the wrong-CAK test) | 248 runs, 0 failed | c96fe45972f07c05 |
| d-trace-rust-g4-2.txt | 11:32Z | D: Rust trace 2 (from AN 0) | 226 runs, 0 failed | c8b15fc8bfe6862e |
| d6-trace-compare.txt | 11:33Z | D: comparison + script | Rust = C++ (G2 and G0) = G0 Rust B: 80 event runs / 22,560 ops, 0 differences | 6800aeb317fb33e4 |
| d7-rust-reboot-progress.txt | 11:33Z | D: reboot on Rust | Rust package tool ok 97.8 s / 101.1 s | 91dc565c518e7da5 |
| d7-rust-reboot-state.txt | 11:35Z | D: reboot | 0 failed units, 4 up, 32 netdevs | 97ff5ee82d497005 |
| d7-rust-reboot-tool-status.json | 11:35Z | D: reboot | = C except re-reads + summary seconds | 0817c869e10c1acb |
| d7-rust-reboot-macsec-up-check.txt | 11:35Z | D: reboot | PASS 15.4 s | 2b20c281d8c2dc02 |
| d7-rust-reboot-sonic-traffic-test.txt | 11:36Z | D: reboot | 0 lost | 845a2499620b152d |
| d7-rust-reboot-macsec-traffic-test.txt | 11:37Z | D: reboot | 0 lost, equal | 3b0a1f17ba463ce8 |
| d8-rust-cold-power.txt | 11:38-11:41Z | D: cold boot (plug off 11:38:42Z, on 11:39:02Z) | ok 97.7 s / 101.9 s | a3471c99a22c6ea5 |
| d8-rust-cold-state.txt | 11:41Z | D: cold | 0 failed units, 4 up | fe0c8caf7038fa85 |
| d8-rust-cold-tool-status.json | 11:41Z | D: cold | = C except re-reads + summary seconds | 53d8302df0065d79 |
| d8-rust-cold-macsec-up-check.txt | 11:41Z | D: cold | PASS 15.6 s | 4a9afdac7600da2d |
| d8-rust-cold-sonic-traffic-test.txt | 11:42Z | D: cold | 0 lost | 8c53d6f0c52ea14c |
| d8-rust-cold-macsec-traffic-test.txt | 11:43Z | D: cold | 0 lost, equal | f45488042f9ca714 |
| d9-rust-macsec-wrap-test.txt | 11:45Z | D: wrap test, static XPN-256 SAs (fresh test keys, deleted afterwards), sets low + wrap (pn 2^32-300) | 600/600 both sets, in_pkts_ok = out_pkts_encrypted, 0 lost across the low-word wrap | 624a817518396247 |
| d10-rust-sai-failures-coldboot.txt | 11:46Z | D: err lines and SAI statuses of the cold boot | no SAI failure beyond the C++ set | 4df8b223a3e06351 |
| d11-rust-stack-restart-macsec-up-check.txt | 11:47Z | D: systemctl restart sonic.target | PASS 16.9 s | ced0b15dacb13891 |
| d11-rust-stack-restart-macsec-traffic-test.txt | 11:48Z | D: stack restart | 0 lost, equal | d2e0448b64643fc7 |
| d12-rust-kill9.txt | 11:48Z | D: kill -9 syncd | "8 engines were left on by a previous process: reset and back in bypass" | c7dffe6eded61456 |
| d12-rust-kill9-macsec-up-check.txt | 11:49Z | D: after kill -9 | PASS 35.2 s after the unit start (G0 C++ 37.2 s) | 944636f9a3c86864 |
| d12-rust-kill9-macsec-traffic-test.txt | 11:50Z | D: after kill -9 | 0 lost, equal | 4b69ce8aed8a78c8 |
| d13-rollback-to-C-activation.txt | 11:51Z | D: rollback to C (switch-to-configuration of z7jccivx) | C++ libsai 7gqia5n3 | babaf8efb9f64207 |
| d13-rollback-to-C-macsec-up-check.txt | 11:52Z | D: rollback | PASS 16.1 s | ac3b80a8ff21efe0 |
| d13-rollback-to-C-macsec-traffic-test.txt | 11:52Z | D: rollback | 0 lost, equal | 1ce02dc22f86f57b |
| d13-rollback-to-C-not-supported-lines.txt | 11:53Z | D: NOT_SUPPORTED / NOT_IMPLEMENTED lines, C++ vs Rust cold boot | identical | 8f6aacaa3d11c816 |
| d14-forward-to-rust-activation.txt | 11:53Z | D: forward to zvih1crw again | Rust libsai j0jjhc94 | 25d24120ffc46d36 |
| d14-forward-to-rust-macsec-up-check.txt | 11:54Z | D: forward | PASS 16.2 s | f3dc1654e2d5b77e |
| d14-forward-to-rust-macsec-traffic-test.txt | 11:55Z | D: forward | 0 lost, equal | af902a180440864c |
| d15-rust-asic-temp-reader.txt | 11:55Z | D: sonic.target stopped 25 s: the Rust openbcm-asic-temp started by syncd stop hook | attached, 8 monitors, file + prom written, fand no failsafe | b893cd44941aaf2a |
| e-final-state.txt | 11:55Z | E: deployed/booted system, tools | zvih1crw (Rust) current = booted = profile | 61a46f4526d946ae |
| e-final-macsec-up-check.txt | 11:57Z | E: final | PASS 17.6 s | 5172e9e2cc8824a5 |
| e-final-macsec-traffic-test.txt | 11:58Z | E: final | 0 lost, equal | db01450f37ae6681 |

Notes (verified unless marked):
- Smoke step 16 FAIL is the smoke program's check, not the library: `sai_smoke.c:1410` compares MAX_TEMP with the TEMP_LIST
  maximum of one get, but both libraries read the monitors afresh per attribute (`sai_temp.cpp` `switch_temp_get`,
  `sai-core/src/temp.rs` `switch_temp_get`); monitor 5 moves 46.2-47.2 C between consecutive reads (b5) and the two runs
  failed in opposite directions (46 vs 47, 47 vs 46). Values plausible (42-47 C vs C++ 43-48 C).
- Rust MDIO tool status: `summary` / first status line says "in 97.7-97.8 s" where `seconds` is 100.5-101.9; the C tool
  writes the final `seconds` (100.9 = 100.9). `mdio-tool/src/app.rs` `finish()` calls `summarize()` before
  `status_flush()`, which sets `seconds`; C `mdio_script_main.c` sets `g_st.seconds` first. Otherwise status.json equal to
  the C tool's (minus pid/times/temperature; poll re-reads vary by run on both).
- Rust syncd journal vs C++ (d2): no "hostif ... on ceN", "port ceN oper up", "rif ...", "virtual router", "trap type ... kept"
  or "diag socket: <cmd>" NOTICE lines (the backend logs to stderr as "sai-rs openbcm: ..."); MACsec model NOTICE lines are
  there (rekey count works). sai_api_query out of range answers INVALID_PARAMETER (documented difference).
  One `[SOC KNET RX]soc_knet_handle_rx: unhandled (type=3, opcode=31)` (KCOM event KCOM_M_DMA_INFO) at the first Rust syncd
  start after the C++ syncd (11:17:09Z); not at the reboot, cold boot, kill -9 or forward-switch starts; meaning not
  established (guess: an event from the KNET module left by the previous process).
- Not run: `loadtest/ring.sh` (SONiC stopped, bcm.user behind bcmd.py: it does not exercise the Rust libsai), smoke
  `--recreate`. Trace method: as G0 (`../rust-g0/trace-compare.txt`); not traced: attach probe/reset, ext-phy sequences, rekeys.

```
13125a44ac892945ccdae15451b72439449b802a5247014d784cda8dfd5f3d7d  a0-baseline-q4nbqvf2-syncd-warnings.txt
04839342b7eaef026a22a0acb3fc216b941981103d42d11306936d242e2020d4  a1-deploy-switch.txt
bb1c5f8420605dcb9697f035a1944a2a01f5effd5687ea32b2928e9f1834e042  a2-C-reboot-macsec-rekey-test.txt
17f8d1402e475ac98c77e6cc20736eee42691f5dce4626e61ae4db39e11381ef  a2-C-reboot-macsec-traffic-test.txt
72eaab84beea73d1383c98f202e526edf8ede906fccba2f07addc94b675e75c3  a2-C-reboot-macsec-up-check.txt
ea5329b9466c16413658100db9be0b58b9629b699f8a6a46fe79bfa8301fa036  a2-C-reboot-sonic-traffic-test.txt
e8811093ea245a99f949aa13b0e967c5a36b946800c2565c70639e384fbd5ed2  a3-C-syncd-warnings.txt
72a838b4e2c0af5ebc7f536164e63d4b440ac21052b4be4e5a0d685db1291dee  a4-C-tool-status.json
fa830c31701a41fe62f6ecbc51de1ade5e3253808496536f40b4eba2814fe6fb  b0-cxx-reference-ps-knet-temp.txt
ccf9aa57eeb94e1c9582d6b672c2b4b97d6511b68fb400d7ea2fff74ef5698af  b1-smoke-noextphy.txt
04afec1204ef46090ce470b4dc7b02cb160ce10cc587049788d0ec029b201339  b2-smoke-extphy.txt
1bc96edf6774faaf41ca8588dfdbc954c9a701e94a2638e08248885812b528bd  b3-smoke-window-journal.txt
0abce6fcdd49aaf015453e6840f57b5295a5a001522614551a6edae282c7afd2  b4-C-after-smoke-macsec-traffic-test.txt
e34c7df1529c2fa812688526be58ec954c36ef51520cf2fedcdc9eb2bc561c3a  b4-C-after-smoke-macsec-up-check.txt
f4a772b5b99cf8cd1e30b9d63ee17faae5a625635980812dd333c09e96ca3ab9  b4-C-after-smoke-sonic-traffic-test.txt
de0635f2d89c07981d2dad447c682f72a49dab9bd770c312a1234c5e3f03d244  b5-cxx-temp-jitter.txt
51354e110cf06a0fce2360f246371af5ac29c33b345c2fc770d1f43b1807520f  c0-deploy-rust-mdio-boot.txt
60cfd766855b07a81508cf23f9b223ad884506a57800b78814b6715cbe191647  c1-rustmdio-reboot-macsec-traffic-test.txt
e9bfac410409a8d3eb1a010271ff8e61989ee6e0cb655366ef6a29e13c118bf3  c1-rustmdio-reboot-macsec-up-check.txt
a217110c9c879346832e9c443f75140ec327df9ccefe08271163e9fdf2507d83  c1-rustmdio-reboot-mdio-status.txt
c374900db429ffe2419e7f0d1e90620a6962d24aca2925a03d5e1a07fc56a126  c1-rustmdio-reboot-progress.txt
e17bbe34467c91317db192e9cbe0334ff9f57923ee4b9890f383f1b4b62d7bb9  c1-rustmdio-reboot-sonic-traffic-test.txt
b36ff97d72ee0678f7049516c96d8e2072aa8ac6daef5e7ba730340b42c1e4e7  c1-rustmdio-reboot-tool-status.json
d049def7619070cf0191ce3db55d06bdbd74cf74c058e942f9d29799df6a822e  c2-rustmdio-cold1-macsec-traffic-test.txt
efc0721a31810af05536dfe8615a4359d4ff9f229c5f3a436ffeda6233c2e11f  c2-rustmdio-cold1-macsec-up-check.txt
5a40847f050c3a25123b2b9fd761cfcb29282fc83c4d9b90e2e117663965375e  c2-rustmdio-cold1-mdio-status.txt
152543bfc7e8b554c33dbe653def842bb2e628106ca11f6d91298d8511d94d6f  c2-rustmdio-cold1-power.txt
a6abeee0fe3657d37f26989e3102c6200dc5224fe03677aa805747727eab8b89  c2-rustmdio-cold1-sonic-traffic-test.txt
a3c0c602a2ffe1967f39e5d42a27248d3c8b61924feaad4acaa121c249ce15c4  c2-rustmdio-cold1-tool-status.json
b6c39b950b63f89e78310e3f7d2c52e13be9802c7fee8f7d1075a87211ce41e8  c3-rustmdio-cold2-macsec-traffic-test.txt
385fbc7640914fc7346371c74f5ac987895ffe761fdeb9d7b9ad34100062f6dd  c3-rustmdio-cold2-macsec-up-check.txt
dfd9a03e2772a7221e593bbf659c788b4085ce1c48379bda0babb4b5e9333c3a  c3-rustmdio-cold2-mdio-status.txt
80d061db204632bdf9d14cb28e3adadb61ee0fbfac13eba8f39feabe661c66f4  c3-rustmdio-cold2-power.txt
951e019571fb68de3525cee4b4c47e467b9b54a2e2e95e1a4c41e1bfed1b308d  c3-rustmdio-cold2-sonic-traffic-test.txt
bcd2701dbcf5ae42eae82ac55a5680943256924949b99956cdee2152fb015872  c3-rustmdio-cold2-tool-status.json
f07d0811983a3798e34dff885718d2f13acebc808ff297ef668a924dba8c43ee  d0-deploy-rust-switch.txt
4df8b223a3e0635125783afb9ce1ed2c920ff725d62da6b6893e24c2aed41a09  d10-rust-sai-failures-coldboot.txt
d2e0448b64643fc74117f4e4e2467d26e97bacdaef5bad0d3d528d7f02c10f13  d11-rust-stack-restart-macsec-traffic-test.txt
ced0b15dacb13891f6bee4542323ff25f9ef3c2a7a5bfc336ae391cd702a2f5f  d11-rust-stack-restart-macsec-up-check.txt
4b69ce8aed8a78c84332ddc9a8f9286ec85676c63a6e60705dfbdd3f04195106  d12-rust-kill9-macsec-traffic-test.txt
944636f9a3c86864d0b98d1798584b2f238e5f3bdcb215b8e4c6faed79584f9f  d12-rust-kill9-macsec-up-check.txt
c7dffe6eded61456b20b5c1e9f9e8180fbd29fb60263fb004bd7dbef92f4851c  d12-rust-kill9.txt
babaf8efb9f64207e246634797db4fcd3637db7c5074af80c48cb761ae9d5d53  d13-rollback-to-C-activation.txt
1ce02dc22f86f57b0c1eb374c0bdfb731eaebdf1cd49ea4c0b8ed0096636e25c  d13-rollback-to-C-macsec-traffic-test.txt
ac3b80a8ff21efe0b90c8dc76dbfac4027ae28f7f9a776d0b4e77ed22adefdde  d13-rollback-to-C-macsec-up-check.txt
8f6aacaa3d11c816f2904ce176671369b5fe111ee67b225aa3ae75cc430100b1  d13-rollback-to-C-not-supported-lines.txt
25d24120ffc46d361f3df07b7377c011286bd373089cef361b16b20879c2a0c4  d14-forward-to-rust-activation.txt
af902a180440864c5ce71495e3d85f0036d2284c54c81a240f19f83cbc3bb48f  d14-forward-to-rust-macsec-traffic-test.txt
f3dc1654e2d5b77eb3b09c7c458b2a941e98f1dcc29b679f0633d814e532faa1  d14-forward-to-rust-macsec-up-check.txt
b893cd44941aaf2a17181fb8edaebe3a4bcbce565727006e242b42274562b176  d15-rust-asic-temp-reader.txt
243074ada6a2653d3eaa3da110ee4458b6424fdc43f4cd37272da21dcd1ac050  d1-rust-syncd-warnings.txt
747a3a76416e5e9525a6d4cba263a8518990dedfdd688de2f721fb074784d337  d2-syncd-journal-cxx-vs-rust.txt
df1e7b722a29140f5b91fde07925da5f23622e80c7f082c966b8dd5f05dc6b84  d3-rust-state.txt
7583638ca11936b597d911929946fe7ffdce4803df696a1b02678c48e29238ef  d4-rust-macsec-traffic-test.txt
57aa3b8bdeb08d52e029537bc650f49076aa2e54a430fe86276cbf3be53cb6e6  d4-rust-macsec-up-check.txt
f99705a09a72c2605a3d3b55a01beb27cdbb3db4bdd34137d33bfe502baf3385  d4-rust-sonic-traffic-test.txt
5ca0e493ee674c79b29c10c4dc2f13ee519ff65cd94baf16bbb3baa7cfde4201  d5-rust-macsec-rekey-test.txt
33dad1d31622e3feab685ce58b80ab06d29abbc2df28e0dbc31caa292b5ada5a  d5-rust-macsec-wrongcak-test.txt
6800aeb317fb33e40abbaf57aec73da70c7e8645391090603662c667f60826d9  d6-trace-compare.txt
3b0a1f17ba463ce8a78fe4025371ac3891a0a70611937bc6d33e90d929791a96  d7-rust-reboot-macsec-traffic-test.txt
2b20c281d8c2dc02c2c955c734278a9d930d28710800bd64bc5f06b48257c489  d7-rust-reboot-macsec-up-check.txt
91dc565c518e7da565f6f3ec54afd0e3849d8e57658cf7e6862eb1b713f2a237  d7-rust-reboot-progress.txt
845a2499620b152db4cc6ca9b7901f55ca4223c204f67729af8601a49cbc302a  d7-rust-reboot-sonic-traffic-test.txt
97ff5ee82d497005324eeed289aaecbd88c966be9e50f8d7253b17c282f839a1  d7-rust-reboot-state.txt
0817c869e10c1acbc154642bd3a082e8b748986fefbf6c0bed787f6ce97b3d33  d7-rust-reboot-tool-status.json
f45488042f9ca7141ce31b6fd529149eaf30128d76393c2279d502ec84723f64  d8-rust-cold-macsec-traffic-test.txt
4a9afdac7600da2db7392ab2d6e696e9a72ec3118b18ff1cb343a46e233b2df1  d8-rust-cold-macsec-up-check.txt
a3471c99a22c6ea5c04e61db344f0c70e9755ddb3a4bc2ecc39456dd94462fb7  d8-rust-cold-power.txt
8c53d6f0c52ea14ca5586d9d75cdbfc53c0a6a9d51fada0448e6407e05692716  d8-rust-cold-sonic-traffic-test.txt
fe0c8caf7038fa855925b95a35d900f1ea04c64de68b11907c9a00fb710e8dc0  d8-rust-cold-state.txt
53d8302df0065d7946d503661fd1d9cfd141666501a621d214e76f868ea38e80  d8-rust-cold-tool-status.json
624a8175183962475ae83ff3739fad9b977aeeebac37b3bd897a869fcd544bc1  d9-rust-macsec-wrap-test.txt
f2dcdc494fa403363d691d7cc24f7a0cf4bf80620ed5a1fce54f68f2a5631db9  d-rust-session-journal.txt
461fadde8b29dd70d309f722d14bec9f37b4d7e3fd7691e3d1d2d0ae0fad230e  d-syncd-journal-boot.txt
f75767c2ea546a5aca157a461f431a590ebe7604192993233119b82c3711d209  d-trace-cxx-g2.txt
c8b15fc8bfe6862e7362cd873a708042f1726a8cd7402ca07709e534de6dbb97  d-trace-rust-g4-2.txt
c96fe45972f07c055ff84fb1a8b5dd91788a44eef00d4dbda61184508ce0494e  d-trace-rust-g4.txt
db01450f37ae66817a72302718bb2efd572d35c9666f9ae0f2cb64ed8a0be398  e-final-macsec-traffic-test.txt
5172e9e2cc8824a55b039dc6fd5db6ca1c0fd7d6e10da17eb09160d302633dd2  e-final-macsec-up-check.txt
61a46f4526d946ae17e9a3fd203ff3e5a94b71e69221a2df31b372bdad914e76  e-final-state.txt
```

# AS7772 G0 hardware baseline for the Rust SAI (agent H, 2026-11-23 UTC)

Gate G0 of the C++ -> Rust SAI migration: committed code only (HEAD `9df9efe`), on the switch, in this order.
Systems (all built from HEAD): **HEAD default** `q4nbqvf2a7hc...` (C++ engine + model, trace hook off);
**B** `qaz8s0kknczx...` (`hardware.openbcm.rustPhySeq` + `rustMacsec`; libsai `d2sj2040...`, markers
`phy-seq-engine` = `macsec-model` = rust); **A** `ljhf30gc33d8...` (`rustPhySeq` only; libsai `20ckkd0x...`).
Before: `j7y63gfr529f...` (C++ without the trace hook). End: HEAD default, rebooted. How each was built and
activated, closure diffs: [deploys.txt](deploys.txt). No PHY hang, no power cycle other than the planned cold boot.

| file | UTC | what | result | sha256 (first 16) |
| --- | --- | --- | --- | --- |
| g0-0-baseline-macsec-up-check.txt | 02:54Z | 0: j7y63gfr, MKA up by itself | PASS, slowest 16.5 s | 96e6d55dcf6c37e4 |
| g0-0-baseline-sonic-traffic-test.txt | 02:56Z | 0: j7y63gfr | 6400 pings 0 lost | 24be77ccec19f907 |
| g0-0-baseline-macsec-traffic-test.txt | 02:57Z | 0: j7y63gfr | 6400 pings 0 lost, encrypted = accepted | d30e0cc8b6980c86 |
| g0-1-head-macsec-up-check.txt | 02:59Z | 1: HEAD default via deploy.sh switch (syncd restart) | PASS, 16.2 s | a35aef46a8a50978 |
| g0-1-head-sonic-traffic-test.txt | 03:00Z | 1: HEAD default | 0 lost | 9e81b81a3b1786df |
| g0-1-head-macsec-traffic-test.txt | 03:01Z | 1: HEAD default | 0 lost, equal | 796c6da794c9fd14 |
| g0-1-head-macsec-rekey-test.txt | 03:09Z | 1: HEAD default, rekey 20 s, 421 s of pings | 210,000 pings 0 lost, 68 egress SA installs | e5779dadf1227100 |
| g0-1-head-macsec-wrongcak-test.txt | 03:10Z | 1: HEAD default (2nd run: the 1st completed on the box, its output was lost by a tool timeout) | no SAs, 0/100, other link 100/100, back in 7 s | 445ec18c34a958fc |
| s0-bundle.txt |  | 2: S0 just sai-rs-test-on (built from the working tree, rust/sai = HEAD then) | 55/55 suites, 291 tests, HARNESS_OK 371 | 9facf35722924e60 |
| s1-mdio-tool-verify-only.txt |  | 3: S1 C vs Rust mdio tool --verify-only via diag socket (160 reads, dev 1) | stdout/stderr/rc and filtered JSON identical | 09a156494c9fdbd4 |
| trace-sessions.txt |  | 4: trace runs (trace on, restart macsecmgrd, 8 gates open, dump): C++ x3, B x2, A x1 | all 0 failed runs, 0 dropped | 95d2f8ce1d49c896 |
| trace-compare.txt |  | 4: comparison of the six dumps | C++ = Rust B = Rust A: 80 event runs / 22,560 bus ops per bring-up, 0 differences | 8d0b072076329b2b |
| trace-digests.txt |  | 4: one line per traced run, all six dumps |  | df69a2ff0461813e |
| trace-cxx-2.txt |  | 4: C++ reference dump (key words ****) |  | 58dfe1ed171b96a2 |
| trace-rustB-1.txt |  | 4: Rust B dump |  | f30d62fd07a37086 |
| g0-4-rustB-macsec-up-check.txt | 03:21Z | 4: B after switch-to-configuration | PASS, 16.2 s | 5bea764c44778b6c |
| g0-4-rustB-macsec-traffic-test.txt | 03:24Z | 4: B | 0 lost, equal | fe454b39c8ea2e2e |
| g0-4-rustB-macsec-traffic-test-2.txt | 03:26Z | 4: B, same SA (counter check) | 0 lost, equal | 5135b5fa653ae34a |
| g0-4-rustB-sonic-traffic-test.txt | 03:24Z | 4: B | 0 lost | f725c6364f9ebaa6 |
| g0-4-rustB-macsec-rekey-test.txt | 03:33Z | 4: B, rekey | 210,000 pings 0 lost, 68 egress SA installs | 35b945c279d5da14 |
| g0-4-rustB-macsec-wrongcak-test.txt | 03:35Z | 4: B wrong CAK | no SAs, 0/100 (+90 ping errors: neighbour resolution, see below), back in 6 s | 0396c76da9475028 |
| g0-4-rustB-macsec-wrongcak-test-2.txt | 03:36Z | 4: B wrong CAK, again | no SAs, 0/100, no errors, back in 6 s | 253aa3f998fdf660 |
| g0-4-rustB-reboot-macsec-up-check.txt | 03:39Z | 4: B after reboot (PHY bring-up 101.1 s, 0 differ) | PASS, 15.4 s | 8504072236ee388a |
| g0-4-rustB-reboot-macsec-traffic-test.txt | 03:40Z | 4: B after reboot | 0 lost, equal | f92bc2e3c569525f |
| g0-4-rustB-reboot-sonic-traffic-test.txt | 03:41Z | 4: B after reboot | 0 lost | dfe4c81393ed70fa |
| g0-4-rustB-coldboot-macsec-up-check.txt | 03:45Z | 4: B after cold boot (poweroff, plug 1 off 20 s, on; bring-up 101.0 s, 0 differ) | PASS, 15.6 s | 4c4384f3bb65fc8a |
| g0-4-rustB-coldboot-macsec-traffic-test.txt | 03:46Z | 4: B after cold boot | 0 lost, equal | 09a6d185ac2ff711 |
| g0-4-rustB-coldboot-sonic-traffic-test.txt | 03:47Z | 4: B after cold boot | 0 lost | 2f1ff59c7702b212 |
| g0-4-rustB-kill9-macsec-up-check.txt | 03:48Z | 4: B after kill -9 syncd ("8 engines were left on ... reset and back in bypass") | PASS, 37.2 s after the unit start | 9aa0ca9b4f194477 |
| g0-4-rustB-kill9-macsec-traffic-test.txt | 03:49Z | 4: B after kill -9 | 0 lost, equal | 099cda1413d6467f |
| g0-4-rustA-macsec-up-check.txt | 03:50Z | 4: A after switch-to-configuration | PASS, 15.6 s | 890345f5b217c867 |
| g0-4-rustA-macsec-traffic-test.txt | 03:52Z | 4: A | 0 lost, equal | c49d8cb70b2f274d |
| g0-5-rollback-macsec-up-check.txt | 03:53Z | 4: rollback to HEAD default (nixos-rebuild, flake pinned to the HEAD rev) | PASS, 16.7 s | 1535f30406ebea4f |
| g0-5-rollback-macsec-traffic-test.txt | 03:54Z | 4: rollback | 0 lost, equal | 6b5097e2b528b186 |
| g0-6-final-reboot-macsec-up-check.txt | 03:56Z | end: HEAD default after a reboot (bring-up 102.5 s, 0 differ) | PASS, 14.8 s | 828dcad258c21096 |
| g0-6-final-reboot-macsec-traffic-test.txt | 03:58Z | end: HEAD default after a reboot | 0 lost, equal | 190f5ceade968175 |
| deploys.txt |  | activation logs, build expression, closure diffs |  | 03fd2daea96f5845 |

Notes (verified unless marked):
- Trace method and the explained differences (periodic PN/counter reads vary in number with time; cross-PHY order
  follows MKA timing; C++ run 1 started from AN 2 on cages 1-2): [trace-compare.txt](trace-compare.txt). Not traced:
  the attach-time probe/reset at syncd start, ext-phy sequences, rekeys.
- Key material: dumps mask key, hash key, salt and IV words (`****`, 256 per dump); the event runs hash
  identically across runs made with different SAKs; the lab CAK was grepped on the switch in every file here: 0.
- `macsec-traffic-test.sh` prints SA counters "since sonic-clear macsec", but they count since the SA was installed
  (both libraries: a second run on the same SA showed 9,624 = 3 runs). Its equality check is unaffected.
- The +90 ping errors in the first wrong-CAK run on B are inferred to be kernel neighbour resolution (ARP entry
  stale, probes blocked by the closed gate); the second run on B did not show them.
- The four full dumps not kept here (C++ 1 and 3, B 2, A 1) are summarised line by line in trace-digests.txt.

```
03fd2daea96f58459528d96160335699c2605a4f8c5d25d383cc220ff06f999d  deploys.txt
d30e0cc8b6980c865df50cd9fc3e5418183687972bab689702f2eb51e8f55aaf  g0-0-baseline-macsec-traffic-test.txt
96e6d55dcf6c37e4466f6f66ced0667e3ddf9b106354ff80a00ce3c83eb40539  g0-0-baseline-macsec-up-check.txt
24be77ccec19f9079b98d3092c36a220c57784cf1fa9922b0f1a096f047c2e70  g0-0-baseline-sonic-traffic-test.txt
e5779dadf1227100a5fcaea923b50cd582065678dff54380e1e8e87856980309  g0-1-head-macsec-rekey-test.txt
796c6da794c9fd14e500b7d5b2a8b76b3e389f110a15c3e316ad1c8fe10a886c  g0-1-head-macsec-traffic-test.txt
a35aef46a8a509781e8bf87bf59b0f80c3f79aff7bd01247cdf6812b151e4895  g0-1-head-macsec-up-check.txt
445ec18c34a958fc8625783e4128df7055c6e2a2499a76c0c99dfb87846757ec  g0-1-head-macsec-wrongcak-test.txt
9e81b81a3b1786dfb467dd60a336fdddd1bf460cdc23b66d765dd233e6413d1c  g0-1-head-sonic-traffic-test.txt
c49d8cb70b2f274d1924a5548babc5ba0e912a84a5401943581de0541597bc0c  g0-4-rustA-macsec-traffic-test.txt
890345f5b217c867277c0edfad067b3b1ceb491fabe8ef369266404559b825cd  g0-4-rustA-macsec-up-check.txt
09a6d185ac2ff7118f24775f9ea80c2471842f0c4b1ec5f8535abd9457e05f6a  g0-4-rustB-coldboot-macsec-traffic-test.txt
4c4384f3bb65fc8a6fd9f57da4dc5891ee2bd67551dcdbda46f81c5bc755f3cc  g0-4-rustB-coldboot-macsec-up-check.txt
2f1ff59c7702b2127d393c607b6580c3117925c22bf2a2e31feaed2c4bdc6263  g0-4-rustB-coldboot-sonic-traffic-test.txt
099cda1413d6467f615bfe996eb73318fc783e7c3fc35824f45a380f83397d20  g0-4-rustB-kill9-macsec-traffic-test.txt
9aa0ca9b4f194477acf172b41d38de1408d610e8ef31b42277aa5f996fb46384  g0-4-rustB-kill9-macsec-up-check.txt
35b945c279d5da1455211ab2bd6631c064929490824156b2ef2add75ab4475c5  g0-4-rustB-macsec-rekey-test.txt
5135b5fa653ae34a8636dfc8c4f1efd67a7bfe5632c8aca955745a568bc0aff1  g0-4-rustB-macsec-traffic-test-2.txt
fe454b39c8ea2e2eb92132c17af1be0ece146bb8f06873e1959b824b42a050c3  g0-4-rustB-macsec-traffic-test.txt
5bea764c44778b6cc6a146274ac8f7eab330c038ee2f698f273a2b48de07cb75  g0-4-rustB-macsec-up-check.txt
253aa3f998fdf6604ce09c9c21a48e4952918c73de13e3b9b91e4f9e3e9f4f51  g0-4-rustB-macsec-wrongcak-test-2.txt
0396c76da947502863b30f156135171acf55f05b5b3344cf91acdd8a13e3786b  g0-4-rustB-macsec-wrongcak-test.txt
f92bc2e3c569525f4b992c5c306c18dbc41e406cb77c90852fc6f524aa9621f7  g0-4-rustB-reboot-macsec-traffic-test.txt
8504072236ee388a114c9621dc2ea3c5e13dc9bd3f174db7270498db0021c6ef  g0-4-rustB-reboot-macsec-up-check.txt
dfe4c81393ed70fa4e96e3d11812b02fb4ee8ade093c2e146e0257ea7c53b89d  g0-4-rustB-reboot-sonic-traffic-test.txt
f725c6364f9ebaa60997e7de0a92c1bf1a32bc43d5293f3bef5f2984531d442b  g0-4-rustB-sonic-traffic-test.txt
6b5097e2b528b1867104d1307e9fc740bc9ff3627a8827761082b6240def836a  g0-5-rollback-macsec-traffic-test.txt
1535f30406ebea4f8defa62cfdd72a64ade4adf0334a95d70da960d1159673ab  g0-5-rollback-macsec-up-check.txt
190f5ceade968175d73d94652799ef0fe926801ca1fd78f8628ebfff2bb7514b  g0-6-final-reboot-macsec-traffic-test.txt
828dcad258c21096b8d0ac4c1cb2c7eb72906f475b37c0756b750aca49dad09f  g0-6-final-reboot-macsec-up-check.txt
9facf35722924e60a963c18bf84e7a5e48ced3bce4ad187c45b69e722a19cbf9  s0-bundle.txt
09a156494c9fdbd428449b580e4f356448a74d38f307cecf58bc116fdbd1f66b  s1-mdio-tool-verify-only.txt
8d0b072076329b2b8114d5f6eb64e95ea405cb38d3a14f180fb21835d2831a3a  trace-compare.txt
58dfe1ed171b96a2ef17dd5af42390962372b2015006c8c9ca1a4fb5add22539  trace-cxx-2.txt
df69a2ff0461813e3d6c2059433db2cbfec4536587d26ceccb8480dcb7c15b6b  trace-digests.txt
f30d62fd07a3708646eb2f591b0a4d656eda877653c2f89476f7ce38b33bd787  trace-rustB-1.txt
95d2f8ce1d49c896682fc4b289c42478d114776af2741be3611c0ede14bef73f  trace-sessions.txt
```

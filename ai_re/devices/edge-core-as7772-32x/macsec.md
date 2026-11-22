# AS7772-32X: MACsec on the Evora (BCM82391) PHYs

2026-11-22. **MACsec works on the chip** (live test, last section but one): vendor mode 2 and our own keys, cages 1-2 and
31-32. **It now runs under SONiC at boot** on the four cabled ports, with MKA and no vendor code on the switch:
[Under SONiC](#under-sonic). The rest of this file is how the engine was found out. Method: static RE of the vendor `bcm.user` (sha256 e163e815…,
addresses are VAs, see [evora-vendor-re.md](evora-vendor-re.md)) plus runs of that code under the re-host
([evora-rehost.md](evora-rehost.md)) on mock backends, then the live test. **V** = read in the code / seen in an emulator run, **I** = inferred.
Tool: `evora-rehost/macsec.py` (+ `lmi.py`, `macsec_test.py`, `macsec-live.sh`).

## Under SONiC
2026-11-22. SONiC's own MACsec stack drives the PHYs: `macsecmgrd` -> one `wpa_supplicant -D macsec_sonic` per port (MKA)
-> APPL_DB -> orchagent's MACsecOrch -> SAI MACsec objects -> sai-openbcm runs **register sequences from board data**
(`nix/modules/boards/as7772/ext-macsec.nix`) on the port's PHY. No vendor code, no emulator and no re-host on the switch.
- **State now:** MACsec (GCM-AES-XPN-256, MKA, lab test key) is **on at boot on cages 1, 2, 31, 32**; the other 28 engines
  are in bypass. System `gcyrw820...`. Config: `nix/configs/as7772/configuration.nix` (`services.sonic.macsec`).
- **Look:** `show macsec [--mka] [Ethernet1/1]` (**prints SAKs**: never paste it unfiltered; the test scripts drop the
  `sak`, `auth_key`, `salt` lines), `sonic-clear macsec`, `openbcm-diag extmacsec` (what each PHY holds: engine, gate, SA per
  AN, the transmitting AN, sequences run / failed).
- **Key:** the CAK is a file on the switch outside the store, `/var/lib/sonic-macsec/lab-test.cak` (0600, one line of 64 hex
  digits, made by hand: `head -c 32 /dev/urandom | od -An -v -tx1 | tr -d ' \n'`). `sonic-macsec-config` reads it at every
  stack start and writes `MACSEC_PROFILE` (SONiC's "type 7" encoding) and `PORT.macsec`. **Without the file the profile is
  not written and the ports run in clear** (the unit fails and says so). The key is then in redis (CONFIG_DB; SAKs in
  APPL_DB and ASIC_DB), as on any SONiC. A new SSD needs the file again.
- **Both ends are this switch:** the far ends (cages 2, 32) get the board's 2nd / 3rd MAC (`INTERFACE mac_addr`), or the
  KaY cannot elect a key server ("duplicate MAC detected"). macsecmgrd waits for those addresses before it starts.

### How it is built
| layer | what | where |
| --- | --- | --- |
| board data | sequences per SAI operation with placeholders for key, hash key, SCI, PN, nonce, window (126 sequences, 22,600 clause-45 operations, generated from 250 lines of Nix: LMI register writes + firmware messages by register) | `nix/modules/boards/as7772/ext-macsec.nix` |
| executor + SAI MACsec API | generic, no chip knowledge: table parser, executor (write, read, compare, poll, keep-and-write-back, named results), `sai_macsec_api_t`, MACsec-stage ACL tables kept as a model, counters accumulated, inter-frame gap 48 B while a port has a MACsec port | `nix/packages/sai-openbcm` (README "MACsec in an external PHY") |
| SONiC | macsecmgrd, sonic-wpa-supplicant, CLI plug-ins, CAK from a file | `nix/modules/switch/sonic/macsec.nix`, `nix/packages/sonic/wpa-supplicant` |
- **PHY layout, fixed per port and direction:** vPort 0 = data (rule 1: every frame, its drop flag is the **gate**), vPort 1 =
  control (rule 0: EtherType 888e, priority 7 -> a BYPASS SA: EAPOL/MKA in clear), SA slot 1 + AN (context id = slot).
  Egress: writing the vPort's SA register (0x4800) makes the new SA the transmitting one. Ingress: the SC's SA table
  (0x4800 / 0x4804, 16 bits per AN) is read and written back with one half changed. One SC per port and direction.
- **Order of things:** MACsec port created = engine out of bypass, EAPOL passes, **data dropped**; first SA + the ACL entry
  that points at the flow = gate open; last SA removed = gate closed first; MACsec port removed = back to bypass
  (classifier low-latency bypass restored too: without that a port drops every frame, found the hard way on cages 1-2).
  A syncd start finds engines a previous process left on (`probe`), empties their SA tables and records and puts
  them in bypass before SONiC builds its own state.
- **Where the sequences come from:** `evora-rehost/macsec.py` gained named objects (`x-vport`, `x-sa`, `x-rule`, `x-rule-upd`,
  `x-devupd`, `x-sa-read`, `x-stats`); five sessions on the real PHYs 0x00 / 0x01 **through syncd with SONiC up** recorded
  every operation with three parameter sets (ciphers rotated over the ANs). `ext-macsec.nix` was written from the first
  recording; `evora-rehost/seqcheck.py` renders the table with sai-openbcm's own executor and compares it with all of them:
  **identical, operation by operation** (4,602 per full session; evidence/macsec-seqcheck-20261122.txt, 70bbf19e2a81).
  The table's hash is pinned in the board module: change `ext-macsec.nix`, re-run seqcheck, update the hash.
  Counter order in the hardware blocks: from the library on a mock that answers reads with a ramp (`EVORA_MOCK_RAMP`).
- Record layout additions to the table above: control word egress `66 e0 4b|4f 92|a2 + 4 x AN` (128 | 256, PN32 | XPN),
  ingress `6f|ef e0|a0 4b|4f d2|e2`; word 1 = slot; XPN: PN high in word 15, nonce (salt with the SSCI XOR-ed into its
  first 4 bytes) in words 17-19, **no SCI in an ingress XPN record**; word 23 (LMI, after the record): `80004000`, ingress
  `+ AN << 29 + slot`. Messages carry an even number of bytes: the last byte of the SA and TCAM messages is never sent.

### Tested on the switch (2026-11-22; evidence/, sha256 prefix)
| test | result | evidence |
| --- | --- | --- |
| S1 static SAs by hand through syncd (`extmacsec run`), SONiC up, cages 1-2, XPN-256 | gate closed: 0 of 400 pings; with SAs: 700 pings (64 / 1400 / 8000 B) 0 lost, encrypted 700 = accepted 700; wrong key: 100 `in_pkts_not_valid`, 100 RFCS at the ASIC, 0 delivered; **link state follows the cable with MACsec on** (far end shut -> down, startup -> up); back to bypass | macsec-s1-static (1205e44e86f7) |
| S2 static SAs through APPL_DB -> MACsecOrch -> SAI (no MKA) | objects created and removed in orchagent's order, SAs installed when the flow is bound, IPG 48 on / 16 off, COUNTERS_DB filled, pings 0 lost, bypass afterwards | macsec-s2-appdb (831e4dd9d587) |
| S3 MKA on both links, `macsec-traffic-test.sh` | 6,400 pings (64 / 1400 / 8000 B, both directions, both links) 0 lost; per SC frames encrypted = frames accepted by the peer; every ingress error counter 0 | macsec-s3-traffic (de6a24bbf435) |
| rekey: `rekey_period` 20 s, 421 s of pings (100,000 each way on cages 1-2, 10,000 on 31-32) | **17 SAK changes per port, AN 0-3 round and round, 0 pings lost**; new ingress SA on both ends, egress 4 s later, old SAs removed 2 s after that | macsec-s3-rekey (40c87a2fa046) |
| wrong CAK (same CKN) on cage 2 | "Computed ICV is not equal to Received ICV", no SAs, both gates closed, 0 of 100 pings; cages 31-32 unaffected; right CAK back: up in 7 s | macsec-s3-wrongcak (aa20bd488b10) |
| `systemctl restart sonic.target` | 8 engines found on, reset; MACsec up again, 6,400 pings 0 lost (the run needed a macsecmgrd restart: the far-end MAC was set after wpa_supplicant started; since fixed by the wait, re-proved by the boots below) | macsec-s3-traffic-stack-restart (8be8bab047e7) |
| reboot | 6,400 pings 0 lost | macsec-s3-traffic-reboot (e44aad0de76c) |
| cold power cycle, final system, unattended | PHY bring-up 99.0 s, **MACsec up on all four ports before anything was run**, 0 failed units; 6,400 pings 0 lost | macsec-s3-traffic-coldboot (e65517d1ba33) |
| S4 load: the ring of [load-test.md](load-test.md) with the PHYs left in MACsec by SONiC (syncd killed, plain SDK, IPG 48), 9 min 20 s | each port **17.75 Mpps, 92.05 Gb/s of frames** per direction (IPG 16 in bypass: 18.59 Mpps, 96.4 Gb/s: MACsec costs 4.5 % with this frame mix); every PHY adds **32.0 B per frame** on the line and removes it again; 9.9 x 10^9 frames per port per direction; RS-FEC 0 / 0, no FCS or error counter anywhere; ASIC 45.7-48.6 C, fans on their curve. **4 of the 740 frames of each ring were lost once** (2 per cable direction; not on the cable: PHY line TX = peer line RX); two more runs (20 s, 90 s) lost none | macsec-load (d34e1092d1a6), -phystat, -samples |
| SONiC back after the load | 8 engines reset at the syncd start, MKA up, 6,400 pings 0 lost | macsec-s3-traffic-after-load (71d4559a5e12) |
Scripts (run on the switch): `macsec-static-test.sh`, `macsec-appdb-test.sh`, `macsec-traffic-test.sh`, `macsec-rekey-test.sh`,
`macsec-wrongcak-test.sh`; parameter files: `evora-rehost/macsec-params.py`, `macsec-appdb.py`. Unit tests: sai-openbcm
`tests/test_macsec.inc` (fake bus: parser, golden rendering, the object model in MACsecOrch's order, key bytes on the bus
and nowhere in the log).

### Limits, not verified
- **The 4 lost frames under load** are not explained (one event in 9 min; three `phystat.py` reads of the PHYs ran during
  that run and none during the two clean ones: a suspect, not a finding). MACsec SA counters were not read under load
  (SONiC was down); the load did not go through SONiC's forwarding.
- Sequences for an SC's **first SA with AN > 0** and its **last SA with AN < 3** are compositions of recorded pieces, not
  recordings (the library cannot be made to allocate those slots first). Both happened on the switch (rekeys, re-MKA
  with AN 1 first) with traffic passing.
- TCAM key word 3 carries a byte of uninitialised library memory in the vendor flow; it is beyond the bytes that are sent.
- One SC per port and direction (point-to-point). Not supported (the SAI call fails): integrity only, SCI not in the
  SecTAG, confidentiality offset, VLAN tag in clear. Replay protection: sequences exist (flow word bit 16 + window),
  recorded and compared, **not exercised with traffic**. Cipher suites other than XPN-256: recorded and compared; XPN-256
  and GCM-AES-128 / XPN (macsec.md live test) seen with traffic, **128-bit and non-XPN not under SONiC**.
- An egress SA that is removed stays in the PHY's record memory until its slot is reused or the engine is reset.
- Between link up and macsecmgrd creating the MACsec port a port is in bypass: frames pass in clear for the first seconds
  of a stack start (the same on any SONiC). MKA to another vendor's switch: never tried (both ends are this code).
- `show macsec` prints SAKs (upstream behaviour). One SAK was shown on the operator's terminal during bring-up; that
  session ended with the next restart.
- No VM test of the SONiC services (the fake backend's ports carry no frames between each other); the unit tests and the
  switch are the proof.

## Result
- **Live:** frames between two PHYs are encrypted (+32 B on the wire, clear frames refused, wrong key = `in_pkts_not_valid`
  and bad-FCS frames the ASIC drops), with the vendor test key, with our GCM-AES-128 keys and with GCM-AES-XPN-256.
  **A bypass-booted PHY switches to MACsec without a firmware download** (`bypass <phy> 0` + `on`), link up throughout.
- The MACsec core is Inside Secure **EIP-163 (classifier) + EIP-164 (transform)**, one pair per direction, driven by the
  Inside Secure CfyE/SecY API behind `bcm_plp_cfye_*` / `bcm_plp_secy_*` (V: strings `DEV_EIP163_0/1`, `DEV_EIP164_0/1`, `ADAPTER_EIP164_*`;
  version registers 0x..fffc read 0x01015ca3 / 0x01045ba4 in the recorded init of the real chips).
- `extphy init … mode=2` installs **one fixed test SA per direction, the same key and SCI for egress and ingress on
  every PHY**, with a match-everything rule: two PHYs back to back interoperate, and it is not secure (key in the binary).
- The library API is mapped and **callable with our parameters** (GCM-AES-128/256, XPN, SCI, AN, PN, replay window,
  confidentiality offset, validate mode, all SA/SecY/interface counters). `macsec.py on <phy> key=… sci=…` with the
  vendor's parameters produces **exactly the MACsec register writes and firmware messages of the vendor's mode 2** (test 3 below).
- The key reaches the chip as plain bytes in MDIO registers 1.a000.. (a firmware message), so an SA install is a short,
  templatable MDIO segment: rekey needs neither the emulator nor vendor code on the switch (design below).

## What `extphy init <id> <n> mode=2` does (V, 0x11745990; `init all mode=2` = 0x117415e0 per bus group)
`mode` is parsed to 0x12f623bc (0x117408f0); the init handler 0x117417f0 branches to 0x11745990 for 2. Per group:
1. `bcm_plp_init(access, rd, wr, method 1)` per PHY: **per-PHY RAM download** (not the broadcast of mode 0; the method is
   hard-coded, `fw_load=` is ignored). Works on a running PHY (26 s through the ssh bridge); returns -24 for a PHY the
   library state already knows.
2. Per PHY, egress then ingress, 0x1173aa80(access, 0): `cfye_device_init` with an all-zero init struct (mode 0 sets
   `flow_latency_bypass` = 1) and `secy_device_init` with all-zero settings except `drop_bypass.drop_type` = 2 (mode 0 sets
   `drop_bypass.fbypass` = 1 instead). Then `secy_config_set` per side. **No `secy_bypass_set`** (mode 0 calls it with 1).
3. `mode_config_set(100000, CAUI)` line + system side (`extphy cfg` afterwards sets CAUI4_C2C + FEC as in mode 0).
4. Per PHY, per side (0 = egress, 1 = ingress), in this order for both sides at each step:
   - `cfye_vport_add` (`pkt_extension` 0 egress / 3 ingress), `cfye_vport_id_get`, `cfye_device_update` (low-latency bypass 0,
     exceptions `drop_action` = 2 internal drop).
   - `secy_sa_add(vport)` with a **static 24-word transform record** (egress 0x12d9debc, ingress 0x12d9de5c) and SCI
     0x12331c8c. Egress: in use, protect frames, include SCI, confidentiality on, offset 0, allow data packets, action
     EGRESS. Ingress: in use, replay protection **off**, validate **STRICT**, AN 0, allow tagged only, action INGRESS.
     Both: drop type internal, destination controlled port.
   - `cfye_rule_add`: packet-type mask 3, data mask 0 = **every packet to that vPort**; `cfye_rule_enable`.
5. `reg_value_set(0x4400005c, 0)` and `(0x4600005c, 0)` for if_side 0 and 1: zeroes word 23 of SA record 0 in both EIP-164s,
   which `sa_add` had set to 0x80004000 (I: the record's SA-update control word; with step 4's roll-over flag the test SA
   then never expires or switches). `macsec.py` does not repeat this for our SAs.
6. Prints "Please send packets now"; the caller then writes 1.826d = 1 (ECC detect, 0x11740720).
- **The test SA** (V; the record is byte-identical to what the library's own SA builder returns for these inputs, context-id
  word 0x6227 aside): GCM-AES-128, key `ad7a2bd0…` (16 bytes at record words 2-5; I: the IEEE 802.1AE Annex C test-vector
  key), hash key = AES_K(0) (checked), SCI `de:ad:be:ef:a5:5a:00:01`, AN 0, 32-bit PN starting 4 (egress) / 5 (ingress),
  ingress window word 0x80, builder flag 8 = "allow PN roll-over". Same key + SCI both directions.
- `rc.soc-MFG` = `init all`, `extphy init all mode=2`, the usual `extphy cfg` lines, taps; `switch_extlb1.sh` adds
  `mac_ipg_48` (switch MAC `AVERAGE_IPG` 48: room for the 32 bytes of SecTAG + ICV per frame).

## The vendor's checks around traffic (V, handler 0x11743660; none of them is a MACsec statistic)
| command | code | what it does |
| --- | --- | --- |
| `reg64 setcrc all RETAIN|NORMAL` | 0x11740ea0 | system-side MAC (if_side 1) reg64 0x41000604 = 0x50_0000c801 / …c800 and 0x41000606 = 0x6408 / 0x640c (I: CLMAC TX CRC mode keep vs append, RX strip CRC off / on), per lane, 100 + 300 ms waits |
| `reg64 clearmibc all` | 0x11740ff0 | writes 0 to every non-zero reg64 0x41000000..0x41000080 (port-MAC MIB), lanes 0-3, both if_sides |
| `reg64 chkmiberr all` | 0x11741390, table 0x12331108 | reg64 0x4100000f and 0x41000053 must be 0 on both if_sides (I: RFCS, TFCS) |
| `reg64 chkecc all` | 0x11741280, table 0x12331088 | 16-bit registers 0x49008010, 0x49009010, 0x4900a010, 0x4900b010, 0x4900c017, 0x4900c099, 0x4900d017, 0x4900d099: bits 1:0 must be 0 (ECC status; breakout: table 0x12330e88, 32 entries) |
The `extphy` shell has **no** command for SA / SecY counters; `macsec.py stats` reads them through the library.

## Library API (V unless marked)
- Dispatch table of function pointers, CfyE from **0x12dc3674**, SecY from **0x12dc36f8**: the member order of the public
  `epdm_sec.h` (`__plp__sec__dispatch__t__`, [evora-public-sources.md](evora-public-sources.md)), with **five empty slots
  after `cfye_vport_index_get`** (so SecY slot = public index + 5). Checked by string (`secy_device_init` 0x12dc36f8,
  `sa_add` 0x12dc3700, `bypass_set` 0x12dc3718, `config_set` 0x12dc371c, `intr_*` 0x12dc3794..), by argument shape of the
  wrappers, and by calling them. Slots used: `SLOT` in `macsec.py`. Beyond `build_transform_record` (0x12dc37b4) the mapping
  is not checked.
- Wrappers (0x1181xxxx) convert the public structs to the Inside Secure ones and call e.g. SecY SA add 0x118c91b0, SA remove
  0x118cab74, SA stats 0x118cc0c4 / 0x118cc34c, SecY stats 0x118c81d4 / 0x118c8400, interface stats 0x118c86cc, RxCAM stats
  0x118c8dc4, bypass set 0x118cd474, `plp_evora_CfyE_*` (names from strings, 0x118be8f8..). They return -25 for a PHY the
  library state does not know (so: `--load-state` of the init run) and 0 / 1 / 2 / 3 = OK / bad parameter / internal / not implemented.
- Structs = public `epdm_bcm_common_sec_defines.h` with one difference found: **`bcm_plp_sec_phy_access_t` is 0x3c bytes,
  `macsec_side` at +0x38** (the embedded `bcm_plp_access_t` has six words, not five). Aggregates such as the SA handle are
  passed by pointer (ppc32 ABI). `bcm_plp_secy_sa_t` 0x28 bytes, `bcm_plp_cfye_rule_t` 0x30, `secy_settings_t` 0x70,
  `cfye_init_t` 0x28. Statistics structs: the library wrote exactly the public field count in every call made on the mock.
- `bcm_plp_secy_build_transform_record(access, bcm_plp_sa_builder_params_t *, out[24])` (0x118183b0): Inside Secure SA builder
  (init 0x118e1c74, build 0x118e1d68). The caller supplies the hash key; `salt_p` != NULL selects XPN (+ `ssci_p`,
  `seq_num_hi`). Flags 2 / 4 (retain SecTAG / ICV) change ingress word 0, flag 8 = roll-over, flag 0x40 is refused.
  **Record layout** (24 words, little-endian byte order inside words): 0 control (AN, direction, key size, XPN), 1 context id,
  2-9 key, 10-13 hash key, 14-15 PN lo/hi, 16 replay window (ingress), 17-19 XPN IV (salt xor SSCI), 20-21 SCI, 22-23 zero.
- Register level: MACsec blocks through LMI (`lmi.py` documents the 1.9005 / 1.900a protocol and decodes traces). Library
  address classes 0x45 / 0x46 = **egress** EIP-163 / EIP-164 (LMI blocks 0xa / 0xb), 0x43 / 0x44 = **ingress** (blocks 2 / 3).
  Arrays (SA record, rule key/mask, counter blocks) do not use LMI: they go as a **firmware message**: bytes packed into
  1.a000.., 1.8222 = 0x1201 (write) / 0x1211 (read), poll 1.8221 == 0x12e3, read data from 1.a101.. (0x118cf2e8 ff.,
  `plp_evora_Device_Write32Array`). The SA write message is 105 bytes: 9 header bytes (0x67 = bytes after the first two, 0,
  engine 1 = egress / 0 = ingress, 1 = EIP-164, 0, word count, 3 address bytes: SA n at n * 0x80), then the 24 record
  words: **key in clear at message bytes 17-48 (0-based), hash key 49-64, SCI 89-96**. One SA add = this message + about 10 LMI writes (ingress: SCI to
  0x4000 / 0x4004, 0x47fc, 0x4800, flow control 0x4a00 / 0x4a04); one `on` of a PHY, both sides, is 1,000 MDIO operations.

## Tool (`evora-rehost/`, scripts only; vendor binary, traces and states are read from `dumps/`)
```
R=devices/edge-core-as7772-32x/evora-rehost; T=devices/edge-core-as7772-32x/dumps/x/evora-trace     # from ai_re/, in the devshell
python3 $R/macsec_test.py                                   # the four offline checks below (3 s)
python3 $R/macsec.py selftest
python3 $R/macsec.py --load-state $T/all-cfg.state --save-state /tmp/x.state --lmi \
    'on 0x00 key=<32 or 64 hex> sci=<16 hex> an=1' 'stats 0x00' 'off 0x00'   # mock: decoded register writes
python3 $R/macsec.py --oracle $T/all-init.trace.txt:0x03 'extphy init 0x00 1 mode=2' 'adopt 0x00' 'stats 0x00'
```
- Commands: `on`, `off`, `sa-add`, `sa-del`, `adopt`, `bypass`, `bypass?`, `stats`, `mib`, `reg`, `devinit`, `diag`, `extphy …` (`--help`).
  Backends: `mock`, `ssh` (private bcm.user, SONiC stopped), `diag` (through syncd, SONiC running; mdio.py). Handles
  are kept in `<state>.macsec.json`; a state that has MACsec objects must be saved by `macsec.py` (it also saves the
  buffers it handed to the library). Keys are never written to traces' section names or the console; they **are** in
  the MDIO trace (1.a0xx words): traces and states stay under `dumps/`.
- Its traces carry `mkscript.py`'s section markers and compile to an MDIO script (checked: 2 PHYs, 1,775 lines; the
  message acknowledge becomes a poll of 1.8221). **Only a trace recorded on the chip may be compiled**: the library
  computes some writes from values it read.
- Mock backends: `ChipMock` = register store + LMI model + message acknowledge; `OracleMock` additionally answers
  reads with what a real PHY answered in a recorded trace (n-th read of a register = n-th recorded value). They prove
  marshalling, call order and return codes, **not** chip behaviour.
- Offline checks (`macsec_test.py`, all pass, 2026-11-22):
  1. SA builder through our marshalling rebuilds both vendor records (words 0, 2-23 identical with flag 8).
  2. The vendor's `extphy init 0x00 1 mode=2` runs to rc 0 on `OracleMock` (download, MACsec init, vPort, SA, rule: every
     "PASSED" line); `adopt` finds its handles (arrays 0x133ec738 vPort, 0x133ed378 id, 0x133eebb8 SA, 0x133edfa0 rule; index id * 8 + side * 4).
  3. Our `on` with the vendor's key / SCI / PN / flags: 136 MACsec register writes + messages = the vendor's last 136
     before its final 0x5c writes, as a multiset (the vendor interleaves the two sides).
  4. AES-256 + AN 1, XPN AES-128 + AN 2 beside it, counters, `off`: all return 0.
- `devinit <phy> macsec|bypass` (uninit + init of the engines without a download) **fails on the mock**
  (`cfye_device_uninit` returns 2: it polls engine state the mock does not model): untested, a fall-back only.

## How MACsec state gets onto the switch (the proposal; built as steps 1-3, see [Under SONiC](#under-sonic))
The boot unit replays a recorded script; SONiC's syncd then owns the SDK and MDIO.
1. **Device init at boot: the bypass script stays.** Stage B showed that `secy_bypass_set(0)` + vPort + rule + SA turn a
   bypass-initialised PHY into a MACsec one at run time, so no mode-2 boot script and no second firmware download are
   needed. The key-less part (bypass off, vPort, rule) can be a second recorded script in `hardware.openbcm.mdioScript.files`
   or part of the SA segments. Note the switch MAC needs `AVERAGE_IPG` 48 on MACsec ports (vendor; all live tests ran so,
   16 was not tried with MACsec on).
2. **Keys: templated SA segments, rendered on the switch.** An SA install is a fixed MDIO sequence per (PHY, direction, SA
   slot) in which only the 24 record words (and for ingress the SCI / AN words) depend on the key. Record each segment
   once on the chip; store it with the record words as placeholders; a small program on the switch (no vendor code: the
   record layout above + AES for the hash key) fills in key, SCI, AN, PN and runs it. Rekey = add ingress SA for the next
   AN on the receiver, then replace / chain the egress SA on the sender, then delete the old ingress SA (the library has
   `sa_chain` / `sa_switch` 0x12dc3758 / 0x12dc375c for a lossless switch; not explored). Needs from the runner: MDIO
   access while syncd runs (it exists: the diag socket; better a syncd-side "run segment" call, since LMI and message
   sequences must not interleave with another user), and a **capture op** in the script format so counters can be read.
3. **Target: SAI MACsec objects in sai-openbcm** backed by those segments, so SONiC's own MACsec stack (wpa_supplicant MKA,
   macsecorch) does key agreement and rekey. 2 is the building block for it.
- Alternative, not preferred: the emulator (Python + Unicorn + the private vendor binary under /var/lib/evora-phy) on the
  switch; simplest to build (`macsec.py` as is, 6 ms per MDIO access through the diag socket), but it puts the vendor
  binary and an emulator in the control path. Static keys inside a recorded script (what `mkscript.py` gives today)
  work for a lab link only: key on disk, no rekey.
- **PN exhaustion decides the design at 100G:** a 32-bit PN lasts 2^32 frames = 29 s at 148.8 Mpps (64-byte frames), about
  9 minutes at 1500-byte frames. Use **XPN** (the builder supports it) or rekey from the PN-threshold event.

## Live test on the switch (2026-11-22, `evora-rehost/macsec-live.sh <stage>`)
SONiC stopped, one `macsec.py --backend ssh` session per stage (PHY commands + our bcm.user for the ASIC: `init all`, one VLAN
per port, `AVERAGE_IPG` 48, CPU `tx`, `show c`). Library state: `all-pre.state` (the booted PHYs), chained stage to stage.
Transcripts: `macsec-live-20261122-*.txt` (no keys); traces, bridge logs (they hold the keys), states and the random test
keys: `dumps/x/evora-trace/macsec-*`. Frame counts: `tx 1000 length=256` on ce0 gives 2000 frames / 508,000 B at ce1 in the
clear, because the ASIC sends the VLAN-101-tagged frame it receives on ce1 back out and it returns untagged (252 B).
| stage | what was done | result |
| --- | --- | --- |
| s0 | baseline, both PHYs as booted | `bypass?` 1 / 1, EIP versions 0x01045ba4 / 0x01015ca3 read live; ce0 -> ce1 2000 / 508,000 B, ce1 -> ce0 4000 / 4,088,000 B, control pair 6000 / 3,060,000 B, no errors (`…-025548-s0.txt`, f08f3c47cfa0) |
| s1 | vendor `init 0x00 1 mode=2` + `cfg`, **cage 2 left in bypass (the tap)** | per-PHY RAM download into the running PHY worked (25.9 s, FW d00b), every "PASSED" line, status "low-latency bypass NO". ce0 -> ce1: **1000 frames, 288,000 B at the ASIC = 256 + 32 B each**, and no returned copy: the ASIC no longer sees the VLAN tag. PHY 0x00: system MAC RBYT 256,000, line MAC TBYT 288,000, SA `out_pkts_encrypted_protected` 1000. ce1 -> ce0 (clear frames into a MACsec receiver): **0 of 2000 reach ce0** (line MAC RPKT 2000, `in_pkts_unicast_uncontrolled` 2000) (`…-025652-s1.txt`, eeeaf0adf284) |
| s2 | vendor mode 2 on 0x01 too (vendor test key) | both directions exactly as s0 at the ASIC; line MACs carry +32 B per frame (572,000 / 4,216,000); encrypted 2000 / 4000 = `in_pkts_ok` on the peer; every error counter 0; `chkecc`, `chkmiberr` rc 0 (`…-s2.txt`, c3ebc1792913) |
| s3 | **our keys**: vendor SAs deleted, K1 for 0x00 -> 0x01, K2 for the other direction, AN 1, own SCIs (`sa-del`, `sa-add`) | identical traffic result, counters on the AN 1 SAs (`…-s3.txt`, 2c919b716f30) |
| s4 | wrong key on the receiver of ce0 -> ce1 | PHY 0x01 `in_pkts_not_valid` 1000, `in_pkts_ok` 0. The PHY does not swallow the frames: it sends them to the ASIC **with a bad FCS** (PHY system MAC TERR 1000, ASIC RFCS.ce1 1000), which drops them. Other direction unaffected. Right key back: 2000 / 508,000 B again (`…-s4.txt`, ae590c59dc05) |
| s5 | GCM-AES-**XPN-256**, AN 2, replay window 128: ingress SA added beside AN 1 on 0x01, then the egress SA of 0x00 replaced | passes; 0x01 ingress AN 2 `in_pkts_ok` 2000, AN 1 0 (`…-s5.txt`, 03690b65e162) |
| B | control pair 31 <-> 32 **from the booted bypass state, no firmware download**: `bypass 0x17e 0`, `on 0x17e …`; traffic; then the same on 0x17f | link stayed up; with only 0x17e switched ce31 receives 3000 x 544 B (512 + 32) and ce30 nothing; with both, traffic passes, encrypted = `in_pkts_ok` (6000 / 3000), no error (`…-stageB.txt`, 9f7777b4c72c) |
| restore | `systemctl reboot` | boot script 98.8 s, 0 expected values differed; `sonic-traffic-test.sh`: 4 ports up, 6400 pings 0 lost, counters equal both ends, 0 failed units (`sonic-20261122-after-macsec-traffic.txt`, d97f7f4417ca). The script's runtime VRFs / addresses are on the box, as after every run of it |
- **Encrypted on the wire** is shown without a tap by s1 and B: one end in MACsec, the other passing the line signal to the
  ASIC, where each frame is 32 B longer (SecTAG with SCI + ICV) and its VLAN tag is gone; and a MACsec receiver drops clear
  frames. No frame was captured: the content itself (EtherType 88e5, ciphertext) was not looked at.
- Counters read through the library clear on read (I, from the sums across stages).
- What went wrong on the way (kept in the transcripts):
  - First s0 (`…-025010-s0.txt`): I had put the CPU port into the test VLANs to capture frames; the second `tx` then hung
    bcm.user (no output for 300 s, process killed). CPU membership removed; no capture was tried again.
  - First s1 (`…-025621-s1.txt`): `init 0x00 1 mode=2` returned -24 without touching the chip, because the loaded library
    state already knew PHY 0x00. The vendor init needs a library that does not know the PHY: s1 runs without `--load-state`
    and s2 adds 0x01 to that state. (The stray `cfg` and the ECC-detect write of that attempt were harmless.)
- Not tested here (some of it since, under SONiC: load at rate, 8000 B frames, the `diag` backend): PN exhaustion, replay
  drops, `off`, `devinit`, break-out, the other 28 cages.

## Not verified / open
- On the chip only what the live table lists was seen. Counter meanings are the public header's names.
- Slot names beyond those exercised; the meaning of SA record word 23; `pkt_extension` 3; the rule words 0x09000000 /
  0x00000101 (copied from the vendor, masked out); the EIP-163/164 register names (addresses only).
- `sa_chain` / `sa_switch` / `sa_nextpn_update`, interrupts / PN-threshold events, control-packet (MKA/EAPOL) bypass rules,
  per-lane (break-out) channels, `devinit`, `off`. Why clear frames into a MACsec receiver count as
  `in_pkts_unicast_uncontrolled` and not as no-tag / untagged is not understood (they are dropped either way).
- Timing: the vendor polls the message acknowledge without a delay or time-out; through a slow bridge that is harmless,
  a compiled script uses a 1 ms poll (mkscript default).

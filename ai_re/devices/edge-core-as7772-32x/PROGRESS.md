# Edge-Core AS7772-32X

## Status
- **Runs:** NixOS 26.11 on the M.2 SSD (Linux 7.3.4) from our U-Boot 2026.10 in boot NOR, **SONiC at boot on the Rust SAI** (`rust/sai`; [sonic.md](sonic.md)); fand, node exporter :9100. Deploy: [deploy.sh](deploy.sh) (flake `#as7772`).
- **Reach:** `sudo ssh root@10.60.2.23`; console `sudo ssh -t console@10.60.2.10 as7760` (DMC-12 port 48, 115200
  8N1, label still `as7760`); power `lab-power 1 on|off`.
- **Recovery:** [u-boot-port.md](u-boot-port.md). Any key stops autoboot; ONIE is still in NOR (`run
  onie_bootcmd`); all NOR partitions dumped. A dead boot NOR needs JTAG or a TSOP-56 rework (no alternate bank).
  NOR writes so far: mtd7 (U-Boot), mtd6 (env). New SSD: `#as7772-sd-image` from ONIE, then
  `evora-rehost/install-script.sh` once (private PHY script with the factory taps, never in git or the store).
- **Ports:** 32 BCM82391 "Evora" PHYs initialised at boot (98 s, 100G MACsec bypass, factory TX taps); cages 1-2
  and 31-32 (DACs) up at 100G, the rest admin-down (their PHY line side powered down): [extphy.md](extphy.md).
  **Link state and LEDs follow the cable** (PHY line-side link polled inside the SDK link scan: far end down ->
  oper down within about 0-0.5 s: bounded by the 250 ms link scan and the 500 ms gate poll; measurement pending). 26 min at line rate, 0 loss ([load-test.md](load-test.md)). **MACsec under SONiC at boot on the
  four DAC ports** (MKA, GCM-AES-XPN-256, lab test key in `/var/lib/sonic-macsec/lab-test.cak`; if the file is
  missing those ports run in clear): [macsec.md](macsec.md).
- **MDIO rules:** `init misc` / `init all` before any external MDIO access; clause 45 **device 1 only**; no
  clause 22; no register sweeps. A corrupted or stray access hangs a PHY until a **cold power cycle**.
- **Boot:** fans 60 % -> 53 % (no failsafe phase); a failed PHY bring-up stops SONiC and says why
  (`systemctl status openbcm-mdio-script`, `/run/openbcm-mdio-script/status.txt`, `phy-status`, login message):
  [sonic.md](sonic.md) "Debugging the PHY bring-up".
- **Quirks:** port LEDs run our program but **nobody has looked at the panel** ([leds.md](leds.md), owner's check);
  after SONiC, a raw-SDK session finds admin-down cages dark (reboot or `extphy power <phy> 1`); do not change
  admin state while a PHY tool runs; PSU2 (empty slot) "NOT OK"; TPM 1.2 left disabled as found
  ([tpm-sensor.md](tpm-sensor.md)).
- **Open:** 28 cages never cabled; nobody has pulled a DAC by hand; frames pass in clear for a few seconds around a stack stop/start on MACsec ports (measured, not closed: [macsec.md](macsec.md)); MKA against another vendor;
  25G break-out; load through SONiC's forwarding, longer soak; USB in U-Boot; CPLD2/3 0x20-0x2b undecoded
  ([cpld.md](cpld.md)); owner's calls: TPM enable, one FEC display convention, whether the PHY register
  sequences in `nix/modules/boards/as7772/ext-phy.nix` may stay in the public repo.
- **Runbook:** phases 0-4 done (no power-cycle-to-U-Boot helper, no network map), 5 done except a long soak, 6-8
  gate passed 2026-11-22. Builds: `just test` (nix/README.md).

## Identity
- Edgecore **7772-32X-A-AC-F-G-HVDC**, PN FP3AO7632043A, S/N **777232X19300200**, label R01A, device version 1,
  mfg 2019-07-30, base MAC 1C:EA:0B:38:2B:D5 (4 MACs), vendor ext `AssetID=3909026511`, diag 0.0.2.7.
  Accton build name **ES7632BT-AO** (the AS7760 is FP3AO7632004A). ONIE platform `powerpc-accton_as7772_32x-r0`.
- CPU NXP **T2080** rev 1.1 (e6500, 4 cores / 8 threads, 1800 MHz), 16 GiB DDR3 ECC (2x AM8D3L16SE11D8NE1).
- Switch ASIC `14e4:b930` x2 functions on PCIe4 x4 gen2 (BCM56930, as on the AS7760); vendor SDK 6.5.9.
  32 x BCM82391 MACsec PHY ports in front of it ([extphy.md](extphy.md)).
- NOR: Intel-ID CFI (0x89/0x227E2801) 128 MiB, 1024 x 128 KiB sectors, the AS7760's 8-partition layout (RCW,
  hw-info, onie 32M, diag 32M, open 62.75M, Fman-FW, uboot-env, uboot). No NAND (the factory banner still says
  "boot from NAND", NAND 0 MiB), no SPI flash, SD slot empty.
- SSD: Intel SSDSCKKR06 (M.2 SATA, 64,023,257,088 B) on SATA2. ONIE's kernel has no GPT support (it sees only the
  protective MBR) and runs with `mem=1920M`.
- PSU1 3Y Power YM-2851F; PSU2 not fitted. CPLD1 (0x60) id 0x01 v4, CPLD2/3 (0x62/0x64) v1, fan CPLD (0x66) id
  0x83 v8 ([cpld.md](cpld.md)). Main board rev G (inferred, [boot-chain-diff.md](boot-chain-diff.md)).

## Deltas from the AS7760 (evidence: `dumps/x/`, unpacked from the NOR dumps)
| what | AS7772 | detail |
| --- | --- | --- |
| RCW | SRDS_PRTCL_S2 **0x15** (AS7760 0x16), one more word (0x...201e vs 0x...2002) and the PBI differ | fits the SATA SSD; not decoded field by field |
| external PHYs | BCM82391 MACsec MAC-PHY on every port | [extphy.md](extphy.md) |
| SDK config | other lane maps, no polarity flips, MDIO keys | [sdk-config.md](sdk-config.md) |
| I2C2 | TPM 0x13, MAX1617 0x18; no `pca9546@77` | [tpm-sensor.md](tpm-sensor.md) |
| CPLDs | CPLD2/3 have registers 0x20-0x2b the AS7760 lacks | [cpld.md](cpld.md) |
| factory U-Boot | 1.7.0.12 (AS7760 1.7.0.10, same day's build); Fman microcode byte-identical | [boot-chain-diff.md](boot-chain-diff.md) |
| diag image | 0.0.2.7 (2019-05-24, Buildroot 2015.02, Linux 3.12.19-rt30); tools in `/usr/local/bin` | AS7760: 0.2.2.9, other layout |
| "open" JFFS2 | only a diag `log/exec_unit.log` ("start command: restart") | |

## Dumps (`dumps/`, git-ignored; `manifest.jsonl`, `SHA256SUMS`, `RAW-SHA256SUMS`; made by [acquire.sh](acquire.sh))
- NOR, source = transfer = reread sha256 for each: `mtd0.bin` 7d4a32ce…, `mtd1.bin` c045a62a… (hw-info TlvInfo),
  `mtd2.bin` e3cc6ada… (ONIE FIT), `mtd3.bin` 39a9b537… (diag), `mtd4.bin` 7d92991c… (open), `mtd5.bin`
  4028d8cb… (Fman), `mtd6-uboot-env-private.bin` 5aead1d5…, `mtd7.bin` 34478cb2… (U-Boot); concatenation
  `nor-full-128MiB-private.bin`.
- SSD: `sda-64GB.img.zst` e7b2e48c… (3.8 MB; raw sha256 a5f54f04…ac9b). **Transfer and reread hashes agree; the
  on-device source hash was skipped** (owner's call, 2026-11-21): two-way, not three-way, verified. Content: GPT,
  one ext4 partition (last mounted `/mnt/ssd1`, journal dirty) with factory test scratch only (`log/exec_unit.log`
  board-temperature test, `traffic.cfg`, empty `post/`, `script/`); no NOS. Since overwritten by NixOS.
- U-Boot env (holds the ONIE dropbear host keys): `uboot-env-private.txt`, git-ignored.
- Board EEPROM: 0x57 behind mux 0x76 ch5 on I2C3, **blank** (70,000 sequential receive-byte reads, all 0xff; no
  address bytes sent, so nothing could be written). `0-0050` in the ONIE tree is a T2080RDB leftover (nothing
  answers). The TlvInfo lives in NOR `hw-info` (mtd1).
- CPLD register dumps (two passes each), PSU1 EEPROM, both SPDs, the device at I2C4 mux ch4 0x25:
  [evidence/i2c-dumps-20261121.txt](evidence/i2c-dumps-20261121.txt). **No CPLD bitstream was read**: the diag ships
  no updater or image and no JTAG path from the CPU is known (`fpga-cpld-playbook.md` section 0).
- Transcripts live in [evidence/](evidence/); serial captures (`serial-*.txt`) are git-ignored.

## Log

### 2026-11-23 (2): Rust-enabled kernel and Rust fan driver are the default; the C fan driver is deleted
- **Running:** generation 31 (`na3yx1c2`): the 47c0012 system + Linux 7.3.4 with `CONFIG_RUST=y` (64-bit big-endian,
  our six-patch series `nix/packages/kernel-rust-ppc64be`) + `as7760_fan` in Rust. Verified on the box. Rollback:
  generation 30 in the boot menu (type its number at "Enter choice:" within 3 s).
- **Proof** ([evidence/rust-kernel/README.md](evidence/rust-kernel/README.md)): ABI conformance module 338/338 twice on the
  real e6500 (8 CPUs); fan sysfs identical to the C driver's (27 lines), pwm writes and rejections as the C on the real
  CPLD; fand never in failsafe; SONiC + MACsec scripts pass; **2 h 3 min at line rate through SONiC with MACsec on**:
  1.30e11 frames per port per direction, 0 discards, 0 errors, MACsec invalid/late 0, ASIC 46.7-48.1 C.
- **Repo:** `hardware.qoriqT2080.rust` defaults to true (the AS7760 gets it too: unproven on its hardware, powered
  off); `as7760_fan.c` deleted, the QEMU test compares the Rust driver with a recording of the C behaviour.
- **Risks on record:** the driver has 11 unsafe blocks (7.3 has no hwmon/SMBus Rust API); rustc warns the
  `-hard-float` target feature this needs will become a hard error (would break the build loudly on a future rustc);
  kernel taint shows W from the ABI module's two deliberate warnings until the next reboot.

### 2026-11-23: the box runs SONiC on the Rust SAI; our C++ SAI for the Broadcom boards is deleted
- **Running:** `as7772-rust` built from `50a6aa7`: syncd on the Rust `libsai.so` (`rust/sai`), Rust `openbcm-diag`,
  `openbcm-asic-temp` and boot-time MDIO tool. Verified on the box (syncd maps the Rust library). The tree after
  this entry (Rust the only implementation, C++ deleted) is **not deployed yet**.
- **Proof on this box** ([evidence/rust-g2/README.md](evidence/rust-g2/README.md), `evidence/rust-g4-load/`): smoke with
  SONiC stopped 23 PASS / 0 FAIL (the first run had 1 FAIL: the smoke's own temperature comparison raced a
  moving sensor; fixed and re-run); Rust MDIO tool as the boot unit over 1 reboot + 2 cold boots, 32/32 PHYs;
  MACsec up-check / traffic / rekey / wrong CAK / PN wrap; MACsec bring-up bus trace equal to the C++ (22,560
  operations); stack restart, kill -9, reboot, cold boot.
- **First load test through SONiC's own forwarding** (Rust SAI, MACsec on, [loadtest/sonic-ring.sh](loadtest/sonic-ring.sh)):
  92.07 Gb/s per port per direction for 30 min, 0 frames lost, encrypted = accepted on every SC. **PHY FEC for that
  run is not established**: `phystat.py` clashes with syncd's MACsec counter reads while SONiC runs with MACsec on.
- **Repo:** `nix/packages/sai-openbcm` is now only `sdk_glue.c/.h`; about 8,500 lines of our C/C++ deleted, about
  23,000 lines of frontend moved into `sai-teralynx/frontend` (the CS8210 still compiles it). Unsafe in `rust/sai`:
  231 -> 143 (82 SAI C ABI, 52 SDK glue, 9 TAP).
- **Open:** deploy the final tree + device scripts + reboot + cold boot; the Rust kernel + Rust fan driver trial;
  the `phystat.py` clash; a benign-by-reading `soc_knet_handle_rx` message (not confirmed on the box).

### 2026-11-22/23 (7): MACsec loss fixed, MKA start-up fixed, Rust phase 2
- **Encrypted-load loss, fixed:** with XPN the 32-bit PN field is 0 at every low-word wrap and the PHY's ingress
  SecTAG check (chip default) rejected that frame: one per 2^32 per SA. One extra write in the ingress SA
  sequences; 2.06e11 encrypted frames over 44 wraps, 0 lost: [macsec-loss.md](macsec-loss.md).
- **MKA start-up, fixed:** MKA did not come up after a stack or syncd restart (the hidden macsecmgrd restart in
  the traffic test masked it; cold boot and reboot always worked). macsecmgrd now starts a port's session only
  when the netdev has its configured address and follows changes (swss patch 0003). Every start path 100 % on the
  final system (3 cold boots, 4 reboots, 5 stack restarts, 3 syncd restarts): [macsec.md](macsec.md).
- **Open, measured:** on MACsec ports frames pass in clear for a few seconds around stack stop and start (engines
  go to bypass before the ports stop forwarding; ND/LLDP/ARP cross before engines are on). Closing it needs SAI
  work + an owner decision.
- **Rust:** phase 2 committed (`rust/sai`): MACsec engine + model in Rust behind default-off switches, agreeing with
  the C++ on this box's CPU; full SONiC fake-backend VM test on the Rust libsai; nothing Rust has driven the chip
  yet. An orchestrated replacement of our C++ SAI by Rust is in progress (`rust/sai/README.md`).
- Deployed system `j7y63gfr...` = HEAD `c9443cf` (the Rust phase-2 commit after it changes the default SAI
  build by a trace hook, not deployed).

### 2026-11-22 (6): MACsec under SONiC; a SAI in Rust started
- **MACsec under SONiC, at boot, on cages 1, 2, 31, 32:** MKA + GCM-AES-XPN-256 through SONiC's own stack
  (macsecmgrd, wpa_supplicant, MACsecOrch) and a generic SAI MACsec layer driven by a board sequence table
  (`ext-macsec.nix`, no vendor code on the switch). 6400 pings 0 lost, rekey every 20 s for 7 min with 0 of
  210,000 lost, wrong CAK blocks only its link, stack restart / reboot / cold boot pass: [macsec.md](macsec.md).
- **Encrypted load:** 92.05 Gb/s of frames per port per direction at IPG 48 (-4.5 % vs bypass), FEC 0/0, ASIC
  46-49 C. **Open defect: 4 of 740 ring frames lost once in the 9 min 20 s run** (not on the cable; two shorter
  runs clean; PHY counter reads during the run are a suspect, not a finding).
- **Rust:** `rust/sai` (first slice: bindings with layout checks, safe core, virtual backend, FFI rim; syncd runs
  against it in a VM; not on hardware yet). Owner's direction: migrate more of the SAI stack to Rust; the MACsec
  sequence engine and object model (C++, proven) are the next candidates.
- Pushed to origin/master through `4af6d13`; the MACsec-under-SONiC commits are local until the owner says.

### 2026-11-22 (5): fans + hard fail, LEDs, cable-true link, code review, all committed
- **Boot:** no failsafe fan phase (60 % then 53 %), the PHY bring-up publishes the ASIC temperature, failure is a
  hard stop with diagnostics; forced-failure and restore proven: [sonic.md](sonic.md). fand gained
  `startup_grace` / `waiting_duty` (commit `7a288ae`).
- **LEDs:** our own program for both LED processors loads at boot; register read-backs match
  ([leds.md](leds.md)). **Not seen by eye.**
- **Link follows the cable:** the Evora passes no link state through, so sai-openbcm gained an external-PHY table
  (link / admin-down / admin-up sequences) hooked into the SDK link scan; far end shut -> oper down in 67-141 ms,
  empty cage stays down, LEDs cable-true. Exploration hung PHY 0x00 and broke the message interface of two PHYs
  (register sweep, devices other than 1): recovered by a cold power cycle; rules added to [extphy.md](extphy.md).
- **Reviews:** U-Boot tree (series as data, guards, NOR hashes pinned in tests), flake/nix (shared lab-access
  profile, SDK config dedup, the "format error on line 3" was our 285-character header vs the SDK's 255-byte
  line buffer, dmc12-netboot evaluates), `just drvpaths | update | uboot-variants`. Finding for the AS7760:
  its NOR (`2c0d7387...`) no longer equals the tree's build (`595d81bf...`), not bisected.
- Everything is on master through the external-PHY commit (not pushed); `just test-eval` 188 passed.
  Deployed system `3bmhqzvs...` = that tree.

### 2026-11-22 (4): MACsec live; docs trimmed, evidence moved
- MACsec proven on cages 1-2 (vendor mode 2, our GCM-AES-128 and XPN-256 keys) and switched on from the booted
  bypass state without a firmware download on cages 31-32; box back in bypass after a reboot, 6400 pings 0 lost:
  [macsec.md](macsec.md). Commit `196efb2`.
- LED programs and Nix wiring written offline, not run: [leds.md](leds.md).
- Cleanup: topic docs cut to facts, transcripts moved to `evidence/`, the old bare-`init soc` MDIO scan scripts
  to `evidence/do-not-run/`.

### 2026-11-22 (3): load test at line rate, factory TX taps, PHY counters
- **Load:** ce0/ce1/ce30/ce31 each 96.4 Gb/s of frames (100.0 % of 100G with IPG 16), 26 min, 29.1e9 frames per
  port per direction, 0 loss by exact counter equality, RS-FEC corrected/uncorrected 0, ASIC 46-49 C. Plain SDK
  rings with SONiC stopped: [load-test.md](load-test.md).
- **Taps:** factory per-lane line-side taps in the boot script (sha256 `e18dddef7c9e...`, cold boot 32/32).
- **Tools:** `evora-rehost` `diag` MDIO backend (through syncd, SONiC stays up) and `phystat.py`.
- Mistakes, detailed in load-test.md: no per-interval rate series (sampler bug); two aborted starts reset the
  chip under traffic (one RFCS/RJBR/TERR each on PHY MAC counters, attributed, not proven).
- Commits `35d3a03`..`0b018e4`, `deee808` (master, not pushed).

### 2026-11-22 (2): SONiC at boot, PHY init on the switch, `just test`
- **Runbook phases 5-8 gate:** cold power cycle to SONiC with no workstation (twice): 0 failed units, 4 ports
  oper-up, 6400 pings over both DACs 0 lost, counters equal on both ends; also after `systemctl restart
  sonic.target` and a reboot. Evidence and hashes: [sonic.md](sonic.md).
- **How:** generic MDIO-script runner in sai-openbcm (`openbcm-mdio-script`, `hardware.openbcm.mdioScript`); the
  Evora script is compiled on the workstation from re-host traces ([evora-rehost.md](evora-rehost.md)). Port
  quirks as sai.profile keys (`SAI_OPENBCM_PORT_{INTERFACE,AUTONEG,FEC,IFG}`).
- **Bugs found:** `boards/as7772/default.nix` merged its fand block with `//`, dropping every `services.sonic`
  line in root-flake builds (the deployed system ran the AS7760 SDK config; `path:./nix` builds hid it): now
  `mkMerge`. Replay timing: the vendor's "1 us" waits must be real waits (10 ms); sleeps under 20 ms are spun
  (4 ms tick). `nix/configs/default.nix` picked the as7760 fan driver by list position: by name now.
- **as7760 neutrality:** system drv `39i9md2c...` -> `7gzhqkh6...`, intended: `nix store diff-closures` shows only
  sai-openbcm (+157 KiB); unit list and sai.profile identical (subagent's check, not repeated by hand).
- **`just test`:** 379 discovered tests, green except real findings: `git:uboot-as5114-chainload` (untracked
  armada8k files) and the dmc12 netboot `bootLoader` attribute (committed bug, not fixed); dmc12 `path:` targets
  are expected failures.

### 2026-11-22: the "silent PHYs" were our MDIO timing; 32 of 32 up, both DACs pass frames
- **What we believed on 2026-11-21:** 11 PHY ids never answer, cage 30 is flaky, id 0x00 was hung by our
  clause-22 / dev-30 test reads, id 0x02 hangs in the vendor init; a CPLD-held reset was suspected.
- **What it was:** our probes and the MDIO bridge ran `init soc` only. On Tomahawk the external MDIO rate and hold
  delay are set in misc init; without it `MDIO_OUT_DELAY` is 3, the edge of the working window (sweep: 0-2
  bit-shifted, 3 = 20/32, 4-15 fine), and a corrupted frame hangs a PHY until a cold power cycle. There are no
  bad PHYs and the CPLDs are not involved. Rule and evidence: [extphy.md](extphy.md); source-level account:
  [mdio-review.md](mdio-review.md). Clause 22 is still not to be sent.
- **Result:** cold power cycle, first access at delay 8: 32/32 ids; `extphy init all` + `cfg` rc 0 on all eight
  groups; ce0/ce1/ce30/ce31 up at 100G, counters equal on both ends of both DACs.
- **Fixed:** `mdio_output_delay=10` next to the divisor in `nix/modules/boards/as7772/sdk-config.nix` (as7760
  system drv unchanged, `39i9md2c...`); the bridge runs `init soc` + `init misc`.
- **Not the cause, checked:** T2080 GPIO1 equals the factory capture (all inputs, data 0x0000aa01); PHY rails
  regulate; the factory boot chain does no PHY/CPLD/GPIO step that ours skips, and CPLD1-3 registers did not
  change through any of it ([boot-chain-diff.md](boot-chain-diff.md), [cpld.md](cpld.md)).
- Cold power cycles in these two sessions (2 + 3): all clean boots.

### 2026-11-21 (4): PHY ID, vendor library re-hosted, first frames over a DAC, TPM + MAX1617, SDK config
- First frames over cages 1 <-> 2 (3000 frames, counters match, no errors):
  `evidence/asic-link-ce0-ce1-20261121-run1.txt`.
- Offline work: vendor `bcm.user` RE ([evora-vendor-re.md](evora-vendor-re.md): ID register 1.8b00, firmware
  extracted to `dumps/x/evora-fw/`), public-source search ([evora-public-sources.md](evora-public-sources.md):
  nothing for Evora), SDK config ([sdk-config.md](sdk-config.md)), TPM + sensor ([tpm-sensor.md](tpm-sensor.md)),
  the re-host ([evora-rehost.md](evora-rehost.md)).
- Deployed the shared T2080 kernel with the ST33ZP24 driver, DT nodes, labels: 0 failed units. Kernel drv
  `5nldvdb9...` -> `9sdiyc9w...` (the AS7760 gets the same kernel: intended, the driver binds only to a DT node);
  as7760 system/image/netboot drvPaths identical with and without the SDK-config change
  (`nixos-build/sdk-config-drvpaths-20261121.txt`).
- **Dead end:** the factory diag image under our U-Boot cannot do SCHAN or MDIO ([extphy.md](extphy.md)).

### 2026-11-21 (3): ASIC attached, die temperature in fand, PHY named
- `hardware.openbcm` enabled. `deploy.sh switch` failed on `openbcm-devices` ("module not loaded"): a new module
  tree is not visible until a **reboot**; after it `BCM56930_B1` attached, 8 monitors, 36.5 C idle, fand uses it.
- The first MDIO scan read only the IEEE ID registers and found nothing (the chip ID is at 1.8b00).
- The factory `config.bcm-100G` differs from the AS7760's in lane maps and polarity on most ports
  ([sdk-config.md](sdk-config.md)).
- CPLD map from the diag's `cpldutil`: [cpld.md](cpld.md).

### 2026-11-21 (2): EEPROM/CPLD dumps, upstream U-Boot in NOR, NixOS on the SSD, fand
- I2C dumps from ONIE (mux 0x76 driven by hand, restored to 0; ONIE's `iic-util` daemon was idle on strace).
- U-Boot: RAM chainload, `ddrcmp` 0 of 50 registers differ, ONIE booted through it, flashed to mtd7, cold and
  warm boots pass: [u-boot-port.md](u-boot-port.md).
- NixOS: `nix/modules/boards/as7772` + `nix/configs/as7772`; as7760 system, image and both U-Boot drvPaths
  unchanged (`20axlar4…`, `25asj5xx…`). Image written to the SSD from ONIE over nc, on-device sha256 of the
  written range equals the image (`59556a01…`).
- **Gotcha (runbook):** the first image did not boot: root had 0 free inodes, so the initrd could not create
  `/etc` ("os-release file is missing"). Fixed in `nix/modules/boards/as7772/ssd-image.nix` (root +1 GiB).
- `saveenv` of `nos_bootcmd`; power-on to login unattended. fand: six sensors, fans follow the curve.

### 2026-11-21: first power-on, inventory, dumps (nothing written)
- Clean cold boot captured: `serial-20261121-190551-initial-boot.txt` (git-ignored, sha256 bc7a918b…). ONIE
  inventory (lspci, mtd, I2C scan of buses 0-12, dmesg):
  [evidence/onie-inventory-20261121.txt](evidence/onie-inventory-20261121.txt).
- Stopped ONIE discovery, dumped NOR over ssh and the SSD over nc, unpacked ONIE FIT, diag multi-image and JFFS2
  offline, compared with the AS7760's originals.

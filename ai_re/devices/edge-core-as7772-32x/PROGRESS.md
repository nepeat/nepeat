# Edge-Core AS7772-32X

## Status
- **Runs:** NixOS 26.11 on the M.2 SSD (Linux 7.3.4) from our U-Boot 2026.10 in boot NOR, **SONiC at boot**
  ([sonic.md](sonic.md)); fand, node exporter :9100. Deploy: [deploy.sh](deploy.sh) (flake `#as7772`).
- **Reach:** `sudo ssh root@10.60.2.23`; console `sudo ssh -t console@10.60.2.10 as7760` (DMC-12 port 48, 115200
  8N1, label still `as7760`); power `lab-power 1 on|off`.
- **Recovery:** [u-boot-port.md](u-boot-port.md). Any key stops autoboot; ONIE is still in NOR (`run
  onie_bootcmd`); all NOR partitions dumped. A dead boot NOR needs JTAG or a TSOP-56 rework (no alternate bank).
  NOR writes so far: mtd7 (U-Boot), mtd6 (env). New SSD: `#as7772-sd-image` from ONIE, then
  `evora-rehost/install-script.sh dumps/x/evora-trace/evora-100g-bypass-taps.mdio` once (private PHY script,
  never in git or the store; the script's default source file is the tap-less one).
- **Ports:** 32 BCM82391 "Evora" PHYs initialised at boot (98 s, 100G MACsec bypass, factory TX taps); cages 1-2
  and 31-32 (DACs) up at 100G, the rest admin-down: [extphy.md](extphy.md). 26 min at line rate, 0 loss
  ([load-test.md](load-test.md)). MACsec proven live, not at boot ([macsec.md](macsec.md)).
- **MDIO rule:** `init misc` / `init all` before any external MDIO access; no clause 22. A corrupted frame hangs
  a PHY until a **cold power cycle**.
- **Quirks (committed tree):** fans at 100 % for the first 2 min 45 s of every boot; syncd starts even if the PHY
  script fails (ports stay down); both being changed by uncommitted edits under `nix/`. Port LEDs dark; PSU2
  (empty slot) "NOT OK"; TPM 1.2 left disabled as found ([tpm-sensor.md](tpm-sensor.md)).
- **Open:** 28 cages never cabled; MACsec at boot / under SONiC; 25G break-out; LEDs (written, never run:
  [leds.md](leds.md)); load through SONiC's forwarding, longer soak; USB in U-Boot; CPLD2/3 0x20-0x2b undecoded
  ([cpld.md](cpld.md)); owner's calls: TPM enable, one FEC display convention.
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

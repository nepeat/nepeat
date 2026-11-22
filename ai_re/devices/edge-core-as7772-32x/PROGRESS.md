# Edge-Core AS7772-32X

## Status
- **Runs:** **NixOS 26.11 on the M.2 SSD** (Linux 7.3.4, shared T2080 kernel), booted unattended by **our upstream
  U-Boot 2026.10 in boot NOR** (`nos_bootcmd = bootflow scan -lb`: extlinux on SSD p1). Mgmt eth0 DHCP
  **10.60.2.23**. 0 failed units, 8 CPUs, 16 GiB. fand runs the fans from the six board LM75s plus the ASIC die temperature
  (`openbcm-asic-temp`, attach only); node exporter on :9100.
- **Reach:** `sudo ssh root@10.60.2.23`. Console DMC-12 **port 48** (consoled label still `as7760`), 115200 8N1:
  `sudo ssh -t console@10.60.2.10 as7760` (ttyS0 autologin). Power: lab plug 1 (`lab-power 1 on|off`).
- **Deploy:** [deploy.sh](deploy.sh) (`nixos-rebuild --target-host`, flake `#as7772`; board module
  `nix/modules/boards/as7772` = the AS7760 board + deltas). First install: `#as7772-sd-image` written to
  `/dev/sda` from ONIE.
- **Bootloader:** [u-boot-port.md](u-boot-port.md). Any key stops autoboot. ONIE 2015.02.00.30 is still in NOR and is
  the fallback behind `nos_bootcmd` (`run onie_bootcmd` from the prompt); it starts installer discovery on every boot.
- **Recovery:** verified dumps of every NOR partition (factory U-Boot `dumps/mtd7.bin`, factory env
  `mtd6-uboot-env-private.bin`) and of the factory SSD content. A working U-Boot prompt can restore both; a dead
  boot NOR needs JTAG or a TSOP-56 rework (no alternate bank).
- **NOR writes so far:** mtd7 (U-Boot, 2026-11-21) and mtd6 (env: `nos_bootcmd`, 2026-11-21). The SSD was overwritten.
- **Sensors / TPM:** MAX1617 at I2C2 0x18 (lm90; remote diode = T2080 die, not a fand input yet) and the ST33ZP24
  TPM 1.2 (`/dev/tpm0`, factory state disabled/deactivated, left so): [tpm-sensor.md](tpm-sensor.md).
- **SONiC at boot:** [sonic.md](sonic.md). `openbcm-mdio-script.service` initialises all 32 BCM82391 "Evora" PHYs
  on the switch (98 s, 100G MACsec bypass, read-back verified) from a private script in `/var/lib/evora-phy/`
  (holds vendor firmware: not in git or the store; a new SSD image needs `evora-rehost/install-script.sh` once),
  then SONiC on sai-openbcm with this board's SDK config and port quirks. **Cages 1 <-> 2 and 31 <-> 32 are up at
  100G and pass traffic after a cold power cycle, unattended**; other ports admin-down. PHYs, port map, MACsec
  notes: [extphy.md](extphy.md). The script is generated on the workstation by the re-hosted vendor library
  ([evora-rehost.md](evora-rehost.md)).
- **MDIO rule:** run `init misc` (or `init all`) before any external MDIO access. After a bare `init soc` the hold
  delay is 3 and frames get corrupted: PHYs then hang until a **cold power cycle**. This was the whole "silent /
  flaky PHY" problem of 2026-11-21; there are no bad PHYs.
- **Known quirks:** fans at 100 % (failsafe) for the first 2 min 45 s of every boot while the PHY script holds the
  SDK; a warm reboot clears the PHYs (the script runs every boot); port LEDs dark (no LED program); PSU2 (empty
  slot) shows "NOT OK"; syncd starts even if the PHY script fails (ports then stay down).
- **Load-tested:** both DACs 26 min at 100 % line rate both ways, 0 loss, 0 FEC errors, factory TX taps in the
  boot script: [load-test.md](load-test.md).
- **Open:** the other 28 cages have never seen a cable; MACsec on (nothing encrypted tried); 25G break-out; LEDs; USB in U-Boot; CPLD2/3 0x20-0x2b
  undecoded ([cpld.md](cpld.md), not blocking); **nothing is committed** (new nix files are `git add -N`).
- **Builds:** `just test` (ai_re/Justfile, `tests/`; nix/README.md) builds every U-Boot variant, package and system.

## Identity
- Edgecore **7772-32X-A-AC-F-G-HVDC**, PN FP3AO7632043A, S/N **777232X19300200**, label R01A, device version 1,
  mfg 2019-07-30, base MAC 1C:EA:0B:38:2B:D5 (4 MACs), vendor ext `AssetID=3909026511`, diag 0.0.2.7.
  Accton build name **ES7632BT-AO** (U-Boot build paths; the AS7760 is FP3AO7632004A).
- ONIE platform `powerpc-accton_as7772_32x-r0`. CPU NXP **T2080** rev 1.1 (e6500 x4 cores/8 threads, 1800 MHz),
  16 GiB DDR3 ECC (2x AM8D3L16SE11D8NE1; ONIE runs with `mem=1920M`).
- Switch ASIC `14e4:b930` x2 functions on PCIe4 x4 gen2 (same ID as the AS7760's BCM56930); vendor SDK 6.5.9.
- NOR: Intel-ID CFI (0x89/0x227e) 128 MiB, same 8-partition layout as the AS7760 (RCW, hw-info, onie 32M,
  diag 32M, open 62.75M, Fman-FW, uboot-env, uboot). U-Boot banner: "boot from NAND", NAND 0 MiB,
  board rev / CPLD ver read as 0xff.
- **SSD:** Intel SSDSCKKR06 (M.2 SATA, 64,023,257,088 B) on SATA2. GPT, one ext4
  partition from sector 2048, last mounted at `/mnt/ssd1`, journal dirty. ONIE's kernel has no GPT support
  (sees only the protective MBR).

## Deltas from the AS7760 (evidence: `dumps/x/`, unpacked from the NOR dumps)
- **RCW:** SRDS_PRTCL_S2 **0x15** (AS7760 0x16), one more word differs (0x...201e vs 0x...2002), PBI differs.
  Fits the SATA SSD; not decoded field by field yet.
- **MACsec external PHYs:** diag `rc.soc-100G` says "external phy, 100G, MacSec bypassed"; `switch_extlb1.sh`
  tests BYPASS, ENCRYPTED and 4x25G channelized modes; `mac_ipg_16/48.soc`. PHY part number not identified.
- **TPM module** at I2C1 0x13; thermal sensor at I2C1 0x18 (diag `i2c_map-T2080-i2c*.csv`, both ACK in the ONIE scan).
- Diag image **0.0.2.7** (2019-05-24, Buildroot 2015.02, Linux 3.12.19-rt30), different layout from the
  AS7760's 0.2.2.9: `/usr/local/bin` has config.bcm-{100G,25G,MFG}, rc.soc-*, port maps, LED programs
  (`ES7632BTx.hex`), I2C maps, fan/PSU/vmon tools.
- U-Boot 1.7.0.12 vs the AS7760's 1.7.0.10 (same day's build). Fman microcode partition is byte-identical.
- "open" JFFS2 holds only a diag `log/exec_unit.log` ("start command: restart").

## Dumps (`dumps/`, git-ignored; `manifest.jsonl`, `SHA256SUMS`, `RAW-SHA256SUMS`; made by [acquire.sh](acquire.sh))
Source, transfer and reread sha256 agree for each NOR partition:
- `mtd0.bin` 7d4a32ce…, `mtd1.bin` c045a62a… (hw-info TlvInfo), `mtd2.bin` e3cc6ada… (ONIE FIT),
  `mtd3.bin` 39a9b537… (diag), `mtd4.bin` 7d92991c… (open), `mtd5.bin` 4028d8cb… (Fman),
  `mtd6-uboot-env-private.bin` 5aead1d5…, `mtd7.bin` 34478cb2… (U-Boot);
  concatenation `nor-full-128MiB-private.bin`.
- SSD: `sda-64GB.img.zst` e7b2e48c… (3.8 MB; raw sha256 a5f54f04…ac9b). **Transfer and reread hashes agree; the
  on-device source hash was skipped** (owner's call, 2026-11-21), so this one is two-way, not three-way, verified.
  Content: factory test scratch only (`log/exec_unit.log` board-temperature test, `traffic.cfg`, empty `post/`, `script/`); no NOS.
- U-Boot env (has the ONIE dropbear host keys): `uboot-env-private.txt`, git-ignored.
- Board EEPROM: `0-0050` in the ONIE tree is a T2080RDB leftover (nothing answers, as on the AS7760). The real
  one is **0x57 behind mux 0x76 ch5 on I2C3 and it is blank**: 70,000 sequential receive-byte reads, all 0xff
  (no address bytes sent, so nothing could be written). The TlvInfo lives in NOR `hw-info` (mtd1).
- CPLD **register** dumps (two passes each), PSU1 EEPROM, both SPDs, the device at I2C4 mux ch4 0x25:
  [i2c-dumps-20261121.txt](i2c-dumps-20261121.txt). Versions: CPLD1 (0x60) id 0x01 v4, CPLD2/CPLD3 (0x62/0x64) v1,
  fan CPLD (0x66) id 0x83 v8. **No CPLD bitstream was read**: the diag ships no updater or image, and no JTAG
  path from the CPU is known, so readback needs the board's JTAG header (`fpga-cpld-playbook.md` section 0).
- There is no other flash: one CFI NOR on IFC (0x89/0x227E2801, 1024 x 128 KiB sectors), no NAND (0 MiB), no SPI
  flash, SD slot empty. PSU2 is not fitted; PSU1 is a 3Y Power YM-2851F.

## Log

### 2026-11-22 (3): load test at line rate, factory TX taps, PHY counters, commits
- **Load:** ce0/ce1/ce30/ce31 each 96.4 Gb/s of frames (100.0 % of 100G with IPG 16), 26 min, 29.1e9 frames per
  port per direction, cable loss 0 and switch loss 0 by exact counter equality, RS-FEC corrected/uncorrected 0,
  ASIC 46-49 C, fand on its curve. Plain SDK rings with SONiC stopped: [load-test.md](load-test.md).
- **Taps:** factory line-side per-lane taps now in the boot script (script sha256 `e18dddef7c9e...`, cold boot
  verified 32/32); eye taller, no measurable error difference on these DACs.
- **Tools:** `evora-rehost` gains a `diag` MDIO backend (through syncd, SONiC stays up) and `phystat.py`.
- Mistakes in the run, kept honest in load-test.md: no per-interval rate series (sampler bug), two aborted starts
  that reset the chip under traffic (one RFCS/RJBR/TERR each on PHY MAC counters, attributed, not proven).
- **Committed** (master, not pushed): `35d3a03`..`0b018e4` (six commits: misc notes, U-Boot, sai-openbcm, AS7772
  NixOS, device docs + re-host, `just test`). Left out on purpose: the AS5114 device dir, the DMC-12 handoff,
  the local APC row in DEVICES.md.
- In progress when written: MACsec live test, fan start-up policy + hard-fail, LEDs.

### 2026-11-22 (2): SONiC at boot, PHY init on the switch, `just test`
- **Runbook phases 5-8 gate:** cold power cycle to SONiC with no workstation (twice): 0 failed units, 4 ports
  oper-up, 6400 pings over both DACs with 0 lost, counters equal on both ends; also after `systemctl restart
  sonic.target` and a reboot. Evidence and hashes: [sonic.md](sonic.md). Checked again by hand after the last boot.
- **How:** generic MDIO-script runner in sai-openbcm (`openbcm-mdio-script`, NixOS `hardware.openbcm.mdioScript`),
  no chip knowledge in it; the Evora script is compiled on the workstation from the re-host traces
  (`evora-rehost/mkscript.py`). Port quirks as sai.profile keys (`SAI_OPENBCM_PORT_{INTERFACE,AUTONEG,FEC,IFG}`).
  The AS7760 `sonic_platform` takes a sensor list; this board passes 8.
- **Bugs found:** `boards/as7772/default.nix` merged its fand block with `//`, dropping every `services.sonic` line
  in root-flake builds (the deployed system had the AS7760 SDK config; `path:./nix` builds hid it): now
  `mkMerge`. Replay: the vendor's "1 us" waits must be real waits (10 ms), and sleeps under 20 ms are spun
  (4 ms tick on the trimmed kernel). `nix/configs/default.nix` picked the as7760 fan driver by list position
  (found by `just test`): by name now.
- **as7760 neutrality:** system drv `39i9md2c...` -> `7gzhqkh6...`, intended: `nix store diff-closures` shows only
  sai-openbcm (+157 KiB); unit list and sai.profile identical (subagent's check, not repeated by hand).
- **`just test`:** discovered targets (379 tests), tiers eval/uboot/packages/systems/deploy/vm/tools; run tier
  by tier, green except real findings: `git:uboot-as5114-chainload` (untracked armada8k files) and the dmc12
  netboot `bootLoader` attribute (committed bug, not fixed); dmc12 `path:` targets are expected failures.
- Owner's calls: fans at 100 % during the PHY script (accept, or publish the ASIC temperature from the tool);
  syncd soft- vs hard-depends on the PHY script; one FEC display convention; what next (LEDs, taps, MACsec).

### 2026-11-22: the "silent PHYs" were our MDIO timing; 32 of 32 PHYs up, both DACs pass frames
- **Cause:** our probes and the MDIO bridge ran `init soc` only. On Tomahawk the external MDIO rate and hold
  delay are programmed in misc init; without it the chip runs MDC at about 17.7 MHz with `MDIO_OUT_DELAY` 3, the
  edge of the working window (sweep: 0-2 bit-shifted reads, 3 = 20/32, 4-15 = fine). Corrupted frames hang PHYs
  until a cold power cycle. The factory always runs `init all` (8 MHz, delay 10). Source-level account:
  [mdio-review.md](mdio-review.md); evidence and the rule: [extphy.md](extphy.md).
- **Result:** cold power cycle, first access at delay 8: 32/32 ids; `extphy init all` + `cfg` rc 0 on all eight
  groups (cage 3 included); ce0/ce1/ce30/ce31 up at 100G, counters equal on both ends of both DACs, no errors.
- **Fixed:** `mdio_output_delay=10` pinned next to the divisor in `nix/modules/boards/as7772/sdk-config.nix` (test
  hash updated, tests pass; as7760 system drv unchanged, `39i9md2c...`); the bridge runs `init soc` + `init misc`.
- Corrections to entry (4): "11 ids never answer", "cage 3 hangs", "cage 30 flaky", "id 0x00 hung by clause 22"
  were all this one cause (clause 22 is still not to be sent). The owner's question "are we doing something
  wrong" was the right one.
- Not the cause, checked: T2080 GPIO1 equals the factory capture (all inputs, data 0x0000aa01); PHY rails
  regulate. The factory boot chain (U-Boot 1.7.0.12 vs the AS7760's, both kernels, scripts)
  does no PHY/CPLD/GPIO step that ours skips, and it explains the vendor diag failure (PCIe window address):
  [boot-chain-diff.md](boot-chain-diff.md).
- Three cold power cycles in this session, all clean boots.

### 2026-11-21 (4): PHY ID, vendor library re-hosted, first frames over a DAC, TPM + MAX1617, SDK config
- **Runbook phase 5 gate at SDK level:** frames cross cages 1 <-> 2 over a real cable, counters match both ways, no
  errors (3000 frames; details and limits in [extphy.md](extphy.md)). Netdevs, load and soak are not done.
- Four offline/side tasks ran as subagents: vendor `bcm.user` RE ([evora-vendor-re.md](evora-vendor-re.md): ID
  register 1.8b00, both firmware blobs extracted to `dumps/x/evora-fw/`), public-source search
  ([evora-public-sources.md](evora-public-sources.md): nothing for Evora), this board's SDK config
  ([sdk-config.md](sdk-config.md), `nix/modules/boards/as7772/sdk-config.nix`), TPM + sensor
  ([tpm-sensor.md](tpm-sensor.md)). A fifth re-hosted the vendor PHY library ([evora-rehost.md](evora-rehost.md)).
- Deployed (new shared T2080 kernel with the ST33ZP24 driver, DT nodes, labels): toplevel `cngcbwap...`, 0 failed
  units. Kernel drv `5nldvdb9...` -> `9sdiyc9w...` (the AS7760 gets the same kernel: intended, the driver binds
  only to a DT node). as7760 system/image/netboot drvPaths are identical with and without the SDK-config change
  (`nixos-build/sdk-config-drvpaths-20261121.txt`); they differ from entry (2)'s only by that kernel.
- **Dead end:** the factory diag image under our U-Boot (`init=/bin/sh`, no NOR/SSD write): its SDK cannot do
  SCHAN or MDIO. Kept: the `extphy` usage text. [extphy.md](extphy.md).
- **Hung PHYs and what we did to them:** id 0x00 hung after our clause-22/dev-30 test reads, id 0x02 hangs in the
  vendor init; both came back with a cold power cycle (two cold cycles tonight, both clean boots).
- CPLD1-3 registers did not change through any of this ([cpld.md](cpld.md)).
- Runbook: phase 4 TPM/sensor done; phase 5 done up to "real links, isolated" on one cable; phases 6 (boot-time
  service for the PHYs) and 8 open. Not committed.

### 2026-11-21 (3): ASIC attached, die temperature in fand, PHY named, first MDIO probe
- `hardware.openbcm` enabled in `nix/modules/boards/as7772` (the `mkForce false` removed). `deploy.sh switch`
  failed on `openbcm-devices` ("module not loaded"): the new module tree is not visible to the booted system
  until a **reboot**; after it, 0 failed units, `BCM56930_B1` attached, 8 monitors, 36.5 C at idle, fand uses it.
- PHY part, MDIO addressing, driver/firmware situation and the probe results: [extphy.md](extphy.md). Short form:
  the vendor names BCM82391 and carries the driver + firmware inside its `bcm.user`; our read-only scan got
  **no ID from any address** (only 0x0000 / 0xffff).
- The factory `config.bcm-100G` differs from the AS7760's in lane maps and polarity on most ports (portmap lines
  are the same), so the "same lanes as the AS7760" spot-check does not hold beyond the portmap.
- CPLD map from the diag's `cpldutil`: [cpld.md](cpld.md). QSFP reset 0x04-0x07, interrupt 0x10-0x13, present
  0x30-0x33 on CPLD1 (presence matches the DACs in cages 1, 2, 31, 32). **CPLD2/3 0x20-0x2b stay undecoded**: no
  vendor tool touches them.
- Not done: subagents for the offline RE (the launch was blocked, nothing ran); the vendor `bcm.user` PHY code and
  firmware; nothing committed.

### 2026-11-21 (2): EEPROM/CPLD dumps, upstream U-Boot in NOR, NixOS on the SSD, fand
- I2C dumps from ONIE (mux 0x76 driven by hand, restored to 0; ONIE's `iic-util` daemon was idle on strace).
  Board EEPROM blank; CPLD registers captured; see Dumps.
- Factory U-Boot inventory, then the port: [u-boot-port.md](u-boot-port.md). RAM chainload first, `ddrcmp` 0 of 50
  registers differ, ONIE booted through it, then flashed to mtd7; cold and warm boots pass.
- NixOS: `nix/modules/boards/as7772` + `nix/configs/as7772`; the as7760 system and image drvPaths are unchanged
  (`20axlar4…`, `25asj5xx…`), as are both as7760 U-Boot drvPaths. Image written to the SSD from ONIE over nc,
  on-device sha256 of the written range equals the image (`59556a01…`).
- **Gotcha (runbook):** the first image did not boot: root had 0 free inodes, so the initrd activation could not
  create `/etc` and switch-root failed ("os-release file is missing"). Fixed in
  `nix/modules/boards/as7772/ssd-image.nix` (root grown by 1 GiB at build time).
- `saveenv` of `nos_bootcmd`; power-on to login unattended. `deploy.sh switch` works in place. fand: six sensors,
  fans follow (100% at start, then down the curve).
- Runbook: phases 0-3 done (no power-cycle-to-U-Boot script kept; the steps are in this entry and u-boot-port.md);
  thermal done except the ASIC die sensor; ASIC phases not started.

### 2026-11-21: first power-on, inventory, dumps (runbook phases 0-1, nothing written)
- Clean cold boot captured uninterrupted: [serial-20261121-190551-initial-boot.txt](serial-20261121-190551-initial-boot.txt)
  (git-ignored, sha256 bc7a918b…). ONIE inventory: [onie-inventory-20261121.txt](onie-inventory-20261121.txt)
  (lspci, mtd, I2C scan of buses 0-12, dmesg).
- Stopped ONIE discovery, dumped NOR over ssh and the SSD over nc (`acquire.sh`), unpacked ONIE FIT, diag
  multi-image and JFFS2 offline, compared with the AS7760's originals.
- Runbook: phase 0 done except the power-cycle-to-U-Boot helper, the fan check and the network map;
  phase 1 done for dumps and offline unpack; U-Boot-side versions, CPLD versions, secure-boot state and
  mining the diag tables are open.

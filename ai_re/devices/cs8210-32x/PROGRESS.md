# CS8210-32X

## Status (2026-11-16)

- **Runs:** NixOS 26.11 (26.11.20261114.2f709fe), Linux 7.3.4, from the SSD, unattended. **SONiC at boot** on our `sai-teralynx`
  (vendor `libifcs.so`, ipd 0.16.2 `legacy_abi=1`): Vrf-a/Vrf-b eBGP + BFD over LACP on the cage 1 <-> 16 400G DAC loop, Credo
  in MACsec bypass ([sonic.md](sonic.md), [sai-features.md](sai-features.md)); only cages 1 and 16 populated (guard refuses syncd otherwise).
  Also: fand; node exporter :9100 on enp0s1; MACsec module (`cs8210-macsec`, MKA; acts under the vendor stack only, [macsec.md](macsec.md));
  sFlow (ipd `psample=1`, sflowmgrd; hsflowd waits for a CONFIG_DB collector, none set). Metrics: job `cs8210-32x` in
  ai-re-metrics, Grafana `ai-re-sonic` ([sonic-metrics-20261116.txt](sonic-metrics-20261116.txt)). Vendor stack: `systemctl start cs8210-switch` (stops SONiC).
- **Reach:** `sudo ssh root@10.60.2.190` (enp0s1). Console: DMC-12 P1 via consoled, `sudo ssh -t console@10.60.2.10 watch cs8210`
  (read-only) / `… cs8210` (attach, `^] w` takes write), `sudo ssh console@10.60.2.10 log cs8210` (scrollback). Power: lab plug 1 (`lab-power 1 …`).
- **Recover:** `systemctl stop sonic.target` or an older generation from the U-Boot menu; hung box or dead I2C FPGA
  (`/run/cs8210-qsfp.dead`): power cycle. Vendor U-Boot prompt: `123<Enter>` within 3 s; from there ONIE in SPI (`run onie_bootcmd`,
  RAM only) or **Route R** into the boot ROM UART CLI, no flash write ([uart-recovery.md](uart-recovery.md), [boot-chain.md](boot-chain.md)).
- **Open:** our U-Boot: RAM chainload boots NixOS + SONiC like the vendor path; the standalone build (own PCIe/MUIO/GPIO/PLL/MAC init)
  passed its RAM rehearsal, never run as stage3's app; permanent scheme (A' install) open, nothing written to SPI ([u-boot-port.md](u-boot-port.md)); **A' installed 2026-11-17 03:10Z**: stage3 boots our standalone U-Boot from SPI slot A, NixOS boots from it, but the MDIO FPGA 1d0f:ec30 has no BARs so the Credo ports (cages 1-16) and SONiC links stay down; `u` escape untested; factory boot = `u` at stage3 or `spi_upgrade.py rollback --write` ([spi-upgrade-plan.md](spi-upgrade-plan.md)); owner decision on
  publishing `ai_re/nix/packages/sai-teralynx/src/ifcs_api.h` (our IFCS declarations from the vendor ctypes + DWARF); AS7760 redeploy of
  the shared SONiC changes (local_asn, teamd MAC, networkd rules); SAI gaps in sai-features.md (WRED/ECN, PFC/lossless, SET_DSCP/remarking,
  sFlow sample policing (isai's ACE impossible on 0.12.41, the CPU-queue meter misbehaves: off), tunnels/IPMC/MPLS/NAT, warm boot,
  BFD offload); MACsec under SONiC; MKA across two boxes; CRM panels empty (crmorch writes no CRM:STATS here, cause open);
  xcvrd reports no module in cages 1/16 (transceiver panels empty); the ASIC's CPU TX ring stopping (then the learn
  thread's all-ones read, now guarded in libsai): prime suspect ASPM L0s on its PCIe link, `cs8210-asic-aspm` not
  deployed yet, soak running ([learn-wedge.md](learn-wedge.md)).

## Identity

- Celestica CS8210-32X, codename Briggs (SSD device tree + diag package). Chassis S/N `C8210F2B02211113T0029` (label photo).
- EEPROM (switch board, 0x56 behind 0x70:2 -> 0x74:0 -> 0x71:7): CS8210-32X-UN-00, P/N R3181-F9003-01, rev R0B-2,
  mfg 2021-04-28 (TH), base MAC 0C:48:C6:7A:55:CE (4 MACs; the ASIC/SONiC MAC is 0e:48:c6:7a:55:ce), BoardSN R3181-G0001-02TH0321110078.
- CPU card: Annapurna Alpine v3, 16x Cortex-A72 (`0x41/0xd08`), 8 GB DDR (2 x 4 GB, SPD at 0x52).
- Switch: Teralynx 7 (IVM77700, PCI `1d98:1b58` rev 0x10); ports 1-16 through Credo CMS50216 MACsec PHYs (MDIO FPGA
  `1d0f:ec30`), 17-32 straight to the cages; cage I2C/sideband through the I2C FPGA `1d0f:8571` ([fpga/](fpga/README.md)).
- Factory boot chain: AL stage2/3 v3.65.1 -> U-Boot 2018.05-alpine_db-v3.65.1.015 -> ONIE 2017.11.015 (FIT at SPI 0x410000).
  CPLDs: system 0x15, SWLED 2, MACLED 1, fan 3/3. Every programmable part and its copy: [inventory.md](inventory.md).
- SSD: WD Ultrastar DC SA210 120 GB M.2 SATA, model `HBS3A1912A4M4B1`, S/N `A069BCC5`, P/N `0TS1653`, FW `M6102100`,
  120,034,123,776 bytes; imaged 2026-10-17 over a USB adapter (now holds NixOS).
- Console port: 115200 8N1, no flow control (confirmed 2026-11-09); pinout and levels not measured by the agent.
- Not physically inspected by the agent (chip markings, NAND/eMMC population): see HISTORY.md 2026-11-09.

## Standing notes

**Hazards and dead ends** (details in [HISTORY.md](HISTORY.md) under the entry named):
- Mainline panics (SError) on an unanswered MMIO read: unimplemented PBS registers through `/dev/mem` (done twice, power
  cycle each; 2026-11-09 (late) "Mistakes"), or a BAR with memory decode off (both FPGAs leave U-Boot so; (latest+1)).
  `cs8210-pcie-errors` (DWC AMBA error response off) makes such reads return all-ones; keep it ahead of any PCIe user ((i)).
- A dead I2C FPGA stalls the whole SoC on every access (trigger unknown; PCIe remove/rescan not tried). All access goes
  through the `cs8210-qsfp` lock; never poll it from a second tool ([sai-hw-fpga-stall-20261116.txt](sai-hw-fpga-stall-20261116.txt)).
- **House network:** on 2026-11-11 a CPU 0 soft lockup on an SDK restart under load (round-2 ipd patches, since removed)
  knocked out the Unifi Aggregation <-> UDM link ((z)). The front ports are cabled only as the cage 1 <-> 16 loop; before
  any cable to the house again do the (z) checklist (lockup found, forwarding at SDK init/teardown guarded, a topology
  that cannot loop). The vendor `-s` single-L2 mode and a Linux bridge over hardware-bridged fpp ports would loop ((ab)).
- `asic_reset` (sys CPLD 0x03 bit 5) is a pulse and cannot hold the ASIC in reset; it was the one correlate of the
  2026-11-10 SError (opt-in now). PMBus OPERATION-off on the ASIC/MACsec rails not attempted (sequencing risk) ((g), (i)).
- Vendor `otp` has password-gated irreversible subcommands: only ever `otp show direct|shadow` (2026-11-15).
- Stage3 console keys must be queued from stage2's banner; sent after `Executing next` they arrive too late (2026-11-15).
- `modprobe` after `switch-to-configuration switch` loads the booted generation's module: insmod by path or reboot.
  `pkill -f` matched and killed my own ssh twice ((c)). A scripted rewrite once truncated this file ((z) note).
- ONIE: its dropbear accepts root without a key (check `uname -r` before running NixOS scripts); no `/sbin` in a
  non-interactive PATH; `fw_setenv` needs `-f`. U-Boot `dhcp` overwrites `serverip`; `bootm` cannot boot mainline
  (Image > `CONFIG_SYS_BOOTM_LEN`) (2026-11-09 (late)).
- TPM works (DT mux on PBS52/54). The aux SPI flash behind the same mux still reads all ones in every mode/speed tried.
  Ruled out for the aux bus before the mux fix: interrupted autoboot, `scsi init`, `booti` vs `bootm`, probe timing, pin
  mux (2026-11-09 (late), (latest)).
- libifcs's learn thread wedges on an all-ones ring index ((n), (o), (y)): vendor stack (overcommit 0) a failed 32 GiB
  malloc, a spin and a log flood (watchdog + logtrim); SONiC (redis sets vm.overcommit_memory = 1) a 32 GiB memset that
  took the box's memory in seconds (2026-11-16). Since: libsai's learn-ring guard, MemoryMax on syncd and cs8210-switch,
  both watchdogs ([learn-wedge.md](learn-wedge.md)).
- Vendor stack only: ASIC traps unicast to the CPU only for SDK-programmed MACs ((t)); `double_CRC` must be off in
  MACsec bypass ((x)).

**Fans** (user request 2026-11-09: keep them quiet). fand holds PWM 127 (~50 %, ~11k rpm) until the hottest LM75 passes
40 C or the ASIC die 70 C, full speed at 55 / 95 C; a fault or stop forces full speed and re-arms the fan watchdog.
`fand status`, `fand set 80% --for 10m`; `cs8210-fan status|set|quiet|max|watchdog` when no daemon runs;
`hardware.cs8210.fan.controller = "python"` is the old fallback. Code: `nix/nixos/module/cs8210/{fan-control,tools}`.
Tach x100 and the rotor maxima come from one fan sample. **Still loud from power-on until NixOS userspace (~40 s):** the
vendor U-Boot never touches the fans; untried U-Boot recipe `i2c dev 3; i2c mw 0x74 0.0 0x04` (top; `0x08` = bottom,
but vendor-uboot.md item 12 has U-Boot's labels the other way round), then `i2c mw 0x66 0x0c 0x18` and
`i2c mw 0x66 0x22 0x7f` ... `0x82`.

**External PCIe INTx** is not delivered on mainline (Annapurna IOFIC has no driver; same on the DMC-12). The ASIC uses
MSI-X (ipd: 8 vectors), so only the FPGAs' AER/INTx is lost: a dead FPGA leaves nothing in the kernel log.

Config: `nepeat/nix` flake output `cs8210` (`nix/nixos/machine/cs8210`, `nix/nixos/module/cs8210`) plus the shared
`ai_re/nix` (sai-teralynx, `boards/cs8210`, `switch/sonic`). The vendor userland lives only on the box
(`/var/lib/cs8210-sdk/root`); the factory DIAGOS exists only as the 2026-10-17 SSD image.
Topic files: [sonic.md](sonic.md), [sai-features.md](sai-features.md), [sai-oracle.md](sai-oracle.md),
[sai-qemu.md](sai-qemu.md), [macsec.md](macsec.md), [macsec-400g.md](macsec-400g.md), [boot-chain.md](boot-chain.md),
[vendor-uboot.md](vendor-uboot.md), [u-boot-port.md](u-boot-port.md), [uart-recovery.md](uart-recovery.md), [inventory.md](inventory.md),
[cpld-diff.md](cpld-diff.md), [fib-capacity.md](fib-capacity.md), [switchd/DESIGN.md](switchd/DESIGN.md), [fpga/](fpga/README.md);
earlier offline work: [ssd-analysis.md](ssd-analysis.md), [kernel-review.md](kernel-review.md),
[offline-research.md](offline-research.md), [board-control.md](board-control.md), [bringup/](bringup/README.md),
[review triage](reviews/2026-10-17-triage.md); superseded: [loopback-plan.md](loopback-plan.md).

## Next useful work

1. Our U-Boot: scheme A' install of `#uboot-cs8210-stage3app` (installer agent) once the AVS oneshot is deployed; quiet fans optional ([u-boot-port.md](u-boot-port.md)).
2. Aux SPI flash gating (the i2cfpga flash reads all ones on mainline); what CPLD v15/v2 changed ([cpld-diff.md](cpld-diff.md)).
3. Credo slice temperatures and the ASC10 voltage/temperature monitors.
4. Populate `/var/lib/cs8210-sdk/root` reproducibly (from the `onie` MTD on the box) instead of by hand.
5. Kernel drivers for the two FPGAs (cage I2C, MDIO) instead of userspace BAR access; port LEDs (dark under SONiC).

## artifacts

- `images/ssd-2026-10-17.img.zst`: full-device image, 1,677,810,900 bytes
  compressed from 120,034,123,776 bytes (98.6% smaller).
- `evidence/`: acquisition metadata, partition table, SMART output, verification.
- `photos/`: original HEIF label photos and converted PNGs.
- `analysis/extracted/`: 856 selected firmware, SDK, diagnostic and identity
  files, 268,351,514 bytes logical size; hashed in `evidence/extracted-manifest.json`.
- `analysis/celestica_cs8210-r0.dts`: decompiled board device tree.
- Temporary raw analysis image was removed after inspection. All mounts and loop
  devices were released; only the zstd full-disk image is retained.
- `scripts/acquire.py`, `scripts/verify.py`: acquisition and verification methods.

Binary images, photos, and raw evidence are local-only and ignored by git. They
can contain device configuration or secrets. Summarized findings belong here.

## log

### 2026-11-17 — MDIO FPGA BAR on a cold boot via our U-Boot: root cause, fix, rescue proven

- Cause: the MDIO FPGA's BAR 0 keeps only bits 31:29 and 21 and resets to 0xe0200000 = its sizing read-back, which Linux
  rejects ("can't size"); the vendor U-Boot writes e0000000 first. Fix (0030/0031): our U-Boot writes the vendor's partner
  BARs once the link is up; RAM-tested from the forced cold value: NixOS, Credo, BGP/BFD/LACP as on the vendor boot.
  Fixed `#uboot-cs8210-stage3app` (.fimg 58e4a4f0…) is for the slot B update; slot A still holds the old one.
- Rescue: power cycle + `u` from stage2's banner → factory U-Boot, three times; warm reset + `u` likewise.
- Hazard: repeated `alpcie reinit 4` wedges the MDIO FPGA link until a power cycle (the factory U-Boot too). Ports
  without a link are now disabled in the OS DTB (Linux panicked on one). Evidence: [uboot-standalone-20261117.txt](uboot-standalone-20261117.txt).

### 2026-11-17 — scheme A' installed in SPI (our U-Boot as stage3's app)

- `spi_upgrade.py install --write` (owner go): env, slot A, BOOT_MODE, TOC2 written and read back == model; cold boot:
  "Loading application to 08000000", "U-Boot 2026.10-cs8210-selfinit", EL2, x0=0; NixOS from it; mtd0ro == new model;
  AVS oneshot enabled the ASIC (COMMAND 0 -> 0x0406, VOUT 0x0333); fand running.
  [spi-upgrade-install-20261117.txt](spi-upgrade-install-20261117.txt).
- Deviation, stopped there: MDIO FPGA 0004:01:00.0 without BARs ("libMdio: resource0 not available"), cages 1/16 down, BGP/BFD/LACP down.
  Standalone U-Boot work item; the `u` escape test and the boot back into ours are pending.

### 2026-11-16 (late) — scheme A' flasher: built, verified, rehearsed; zero SPI writes

- Tools: `scripts/spi_upgrade.py` (model = verified mtd0 dump + journal, gates, echo-checked writes body-then-header, TOC2 last),
  `scripts/bootchain_emu.py` (factory stage2 + stage3 in Unicorn), `nix/packages/alboot/alboot3.py`, patches 0027/0028
  (`#uboot-cs8210-installer`: SPI writable only in TOC2/BOOT_MODE/env/slot A/slot B). Layout and safety case: [spi-upgrade-plan.md](spi-upgrade-plan.md).
- Slot B moved to 0x280000 (1 MiB; the 0x2f9000 hole left 1444 B for the standalone build); our env 0x38000+0x8000 erased by install.
- Evidence: 213/213 emulated flash states (every cut state factory or ours, `u` → factory; the BOOT_MODE trap hangs even with `u`)
  [spi-upgrade-verify-20261116.txt](spi-upgrade-verify-20261116.txt); sandbox install/update/switch/rollback + 11 refusals + 32 power cuts
  [spi-upgrade-rehearse-20261116.txt](spi-upgrade-rehearse-20261116.txt); `#uboot-alpine-v3-tests` installer-window.
- Two independent reviews: 17 findings fixed (kill page before erasing a live header, magic written last, no power cycle after
  a failed write, rollback without NixOS, ...); list in the plan's "Reviews". Gates (review, payload, recovery, owner_go) still null.
- Hardware, read-only (lease spi-flasher): /dev/mtd0ro == model; dry runs 17:07 and (final code) 18:38: installer at EL2,
  flash == model, upload ok, writes printed only, power cycle, NixOS + SONiC (BGP 2/2, BFD 2/2), lease released. A first attempt (17:04) matched the consoled scrollback replay: it typed
  `123`, `echo …` and `version` into the NixOS serial root shell (harmless) and then failed on `lab-power` usage; fixed (plan doc).
- Linux mtd0 is read-only (flags 0x800): no Linux-side writer or rollback. DMC-12 pins unchanged (d3df4b80, 2b9lljly).

### 2026-11-16 — learn-thread wedge: guard in libsai, ASPM L0s on the ASIC link, both stacks memory-capped (not deployed)

- sai-teralynx shim guards libifcs's learn-ring reads (GOT: node_pci_reg_read32/_write32, im_malloc): all-ones or
  out-of-range index re-read, else the poll is skipped; im_malloc > 1 GiB refused; journal line + `sai_teralynx_ifcs_guard_*`.
  Mock t_learn (48 checks, 1015 total) and chip (real libifcs, injected all-ones: no 32 GiB, BFD up).
- First failure on 2026-11-16 was the CPU TX ring: ASIC stopped sending ~16:01, ring exactly full (341 frames x 3
  descriptors) at 16:03:45 = the softirq spin; the bad read came 2 min later. The ASIC link had ASPM L0s on (mainline's
  DT default): replay timeouts/BadTLP/rollover every second; none with L0s off (A/B). New `cs8210-asic-aspm` (default),
  cs8210-switch MemoryMax 3 GiB + OOMPolicy=stop (+ watchdog restart), watchdog counters + `cs8210-ifcs` producer.
  [learn-wedge.md](learn-wedge.md), [evidence](sai-hw-learnwedge-20261116.txt). Deploy restarts syncd.

### 2026-11-16 — syncd OOM at 16:05: root cause; syncd MemoryMax, new IFCS watchdog (not deployed yet)

- Cause (high confidence): libifcs's learn thread read a garbage ring index; its im_malloc is malloc + memset, and
  under SONiC's vm.overcommit_memory = 1 (nixpkgs redis `vmOverCommit`) the ~32 GiB request succeeds, so the memset
  OOMed the box in <= 3 s (syncd VmSize +32 GiB to the kB; no `__vm_enough_memory` line, hence no watchdog). The ASIC's
  CPU path had died at 16:00:55 (BFD, LACP and its port TX counters stopped; ipd softirq on 2 cores from 16:03:45).
  PMBus/AVS not implicated (last transaction 3 min before, rail nominal throughout). Trigger of the ASIC failure: open.
  Evidence + timeline: [oom-20261116.txt](oom-20261116.txt).
- `nixos/module/cs8210/sonic`: syncd MemoryMax (`syncdMemoryMaxMiB`, 2048) + OOMPolicy=stop; `sonic-cs8210-ifcs-watchdog`
  rewritten in Python (VmSize, RSS, learn-thread CPU, the kernel line, tx-stall of all transmitting ports; 48 checks at
  build). Box: a 64M transient unit doing the same malloc + memset was memcg-killed in 103 ms, box untouched; the
  watchdog dry-run on live SONiC stayed silent. [sonic.md](sonic.md), [avs.md](avs.md) (no ad-hoc PMBus surveys).
- Deploy note: the syncd unit changes (a switch restarts syncd).

### 2026-11-16 — our U-Boot does the vendor board init (standalone build, RAM rehearsal)

- Patches 0029-0031: SoC init from the DT (MUIO, 32 gpio-list lines, SB PLL ch 9, base MAC from the TlvInfo EEPROM),
  external PCIe RC driver on its own V3 HAL copy (AVG SerDes iCal, vendor fallback), CS8210 DT; `#uboot-cs8210-selfinit`
  (+ SPI env at 0x38000/0x3c000) and `#uboot-cs8210-stage3app` (1444 B spare in A' slot B). Chainload build unchanged (4bcabba8).
- From the vendor prompt: every part 0 changed; all three links torn down and trained by our init, then NixOS + SONiC
  with lspci identical to the vendor boot; PSCI reset ok. Zero SPI writes. Evidence: [uboot-standalone-20261116.txt](uboot-standalone-20261116.txt).
- U-Boot does not auto-configure the link partners (see 2026-11-17 for the MDIO FPGA BAR). ASIC decode is off at
  handoff (the vendor's 6 came from its AVS): the AVS oneshot enables the device itself.

### 2026-11-16 — ASIC AVS as a Linux oneshot (not deployed yet)

- `cs8210-asic-avs.service` (`nixos/module/cs8210/avs`): fuse words via sysfs resource0 -> the vendor decode
  (disassembly-checked: RoV, lot list, table, closest step) -> TPS53688 0x62 VOUT_COMMAND only if it differs, read back,
  `/run/cs8210-avs.done`, `cs8210_asic_avs_*` metrics. syncd and cs8210-switch `Requires=` it; ipd is now blacklisted for
  udev and loaded by this unit (IO_STRICT_DEVMEM blocks the BAR mmap while ipd owns it). Design and readings: [avs.md](avs.md).
- Chip: BAR read = the U-Boot-logged words (RoV 0, lot PS7R09, 800 mV / 0x0333), "already correct", no write; a same-value
  rewrite proved the write path; SONiC back up (BGP + BFD both VRFs). Regulator power-on default: not readable without a
  cold boot. Evidence: [avs-chip-20261116.txt](avs-chip-20261116.txt).
- During read-only work before that test, syncd was OOM-killed at 16:05:48: the learn-thread wedge, not AVS (entry above).
- Deploy note: the syncd and cs8210-switch units change (a switch restarts syncd); the box already holds the fuse record
  the late path needs.

### 2026-11-16 (late) — sFlow trap group, egress sFlow, metrics for the CS8210 (not deployed yet)

- isai oracle `sflow-police`: SAMPLEPACKET = system trap MIRROR_TO_CPU + an ACE on the sFlow collector (MIRROR_2) with the
  group's meter. 0.12.41 refuses a MIRROR match in any ingress profile (adding it to the CoPP profile broke the CoPP table:
  BGP/BFD down until reverted, then restored). tl_sflow: the trap's ENABLE as isai; samples on CPU queue 2 via the init yaml
  (`traffic_monitor_cpu_queue`, board default now); the CPU-queue meter as policer did not hold its rate (kept off,
  `SAI_TERALYNX_SFLOW_METER`). Evidence: [sai-hw-sflow-20261116.txt](sai-hw-sflow-20261116.txt) (22:00Z / 23:02Z sections).
- Egress sampling works with node EGRESS_SFLOW_ENABLE set before node enable (`SAI_TERALYNX_EGRESS_SFLOW=on`, board default;
  the yaml key is ignored): 1 in 9.2 at rate 10, oif right; samples cut to 176 B. No side effect seen (ports, LACP, BGP, BFD).
- Metrics: bgpmon over all VRFs (routing patch 0006, NEIGH_STATE_TABLE|<vrf>|<ip>; bgpcfgd tests 2797 pass, vm-metrics 51/51);
  sonic-metrics: queues, CRM, sFlow collectors; dashboard rows Queues, CRM, sFlow, MACsec. The SONiC Credo unit now preloads
  the MACsec shim (next boot) so `cs8210_macsec_*` has data under SONiC. Evidence: [sonic-metrics-20261116.txt](sonic-metrics-20261116.txt).

### 2026-11-16 — upstream U-Boot 2026.10: milestone 1 (offline)

- `#uboot-cs8210` = patches 0024 (`alpine-v3` target), 0025 (read-only SPI NOR also passes the 4-byte reads the 32 MiB flash needs), 0026 (`cs8210` DT + defconfig): console, 4 DRAM banks, timer, SPI NOR read-only, env in RAM, AHCI SSD, booti/extlinux/boot.scr/bootmenu, PSCI reset. Checklist and runbook: [u-boot-port.md](u-boot-port.md).
- `#uboot-alpine-v3-tests` pass: static checks, QEMU (Cortex-A72, EL2) through the NixOS launcher's boot.scr → bootmenu → booti, MMU-on `go`, PSCI reset. DMC-12 builds unchanged (stage3app `.fimg` d3df4b80, firmware-payloads drv 2b9lljly).
- Read from Linux (read-only): CNTFRQ_EL0 = 75 MHz (firmware-set), PSCI v0.2 over smc, "started at EL2", SSD on AHCI 00:0b.0 port 3, /dev/mtd0/1/4 crc32 = the 11-15 dumps.
- Milestone 2 on hardware (14:42-14:59, RAM only, lease `uboot-m2`): vendor prompt → loadx (0 retries) → `booti` → ours at EL2; SPI NOR crc32 mtd0/1/4 = the dumps incl. above 16 MiB; boot.scr → bootmenu → NixOS: SONiC BGP/BFD/LACP up, dmesg/lspci/DT/AVS as on the vendor boot apart from U-Boot leaving 00:06.0/00:10.0 decode on ([uboot-m2-20261116.txt](uboot-m2-20261116.txt)). Harness fixes on the way (banner lookup, autologin prompt). Back on the vendor boot, healthy, lease released.

### 2026-11-16 — SONiC at boot (generation), bgpcfgd local_asn

- Generation: `hardware.cs8210.sonic.enable`, `autoStart` now the default. Boot order: pcie-errors -> cage/FPGA guard -> Credo -> syncd (waits for /dev/ipd). Nix base CONFIG_DB: Ethernet0/120 admin up with RS FEC, the other 30 down. The vendor stack's bond0/vrf-fp are off with SONiC; `cs8210-switch` stays a `Conflicts=` alternative ([sonic.md](sonic.md) "Running it").
- bgpcfgd: BGP_NEIGHBOR `local_asn` (+ `local_as_no_prepend`/`_replace_as`) at creation and live (patches 0004/0005, not upstream). The loop configs need no vtysh: eBGP on both sides, one-hop BFD, STATIC_ROUTE blackholes advertise the loopback subnets. Tests: bgpcfgd suite 2794 passed (3 upstream BFD-passive cases deselected), vm-bfd 120/120 (local_asn, teamd MAC), chip.
- Chip, hand-run before the deploy: BGP/BFD/LACP up with no vtysh, local_asn removed and restored live, orchagent kill -> syncd restart in 15 s, reload without a saved file = the Nix base ([evidence](sai-hw-sonic-boot-20261116.txt)).
- Found and fixed: a teamd respawn reset the PortChannel's mac_addr (routing over it stayed dead); a networkd restart deletes vrfmgrd's local rule, which leaves the host unreachable. I caused a 25 s outage checking that (the hand-run had left the rule at pref 1001); restored over the console.
- Deployed (5449948, bb38f92, dd0ccb5), two reboots: SONiC up unattended, ssh at ~45 s uptime, guard + Credo + syncd by 51 s, BGP/LACP at ~62 s, BFD at ~64 s, 0 failed units both times. From boot: orchagent kill -> back in 15 s; stop sonic.target leaves fand at 49.8 % with no faults, start again -> BGP in 50 s; a networkd restart keeps the local rule. `sonic-handrun.sh down` now restores the local rule, removes its netdevs and restarts the host lldpd.
- Found after: teamd's debug "usock: calling method StateDump" was ~124 of ~140 journal messages/min and /var/log/swss *.rec grew on the SSD. Fixed the same day: teammgrd `LogLevelMax=info` (17bc832), records on tmpfs `/run/swss` (22d394d); on the running boot 0 StateDump lines and ~36 journal lines/min.

### 2026-11-16 — SONiC on our own SAI: full stack + 2 h soak on the chip

- Owner decision: native SAI (sai-teralynx), all features ourselves. Phases A–C: [sai-features.md](sai-features.md) (chip/mock/missing per object), evidence `sai-hw-*-20261116.txt`, isai oracle [sai-oracle.md](sai-oracle.md); Marvell isai under qemu kept as a reference ([sai-qemu.md](sai-qemu.md)).
- SONiC hand-run ([sonic.md](sonic.md), `scripts/sonic-handrun.sh`): VRF eBGP + BFD over the 400G loop on ports and on LACP PortChannels, ACL drop, DSCP->queue, CoPP ARP policer, counters + CRM in COUNTERS_DB. 2 h soak, 11 flaps: BGP/BFD back 1–7 s after each, no loss outside flaps, daemon memory flat ([sai-hw-sonic-soak-20261116.txt](sai-hw-sonic-soak-20261116.txt)).
- **FPGA stall** ([sai-hw-fpga-stall-20261116.txt](sai-hw-fpga-stall-20261116.txt)): the I2C FPGA (0002:01:00.0) stopped answering (PCIe completion timeouts; each access then stalls the whole SoC for seconds, all-ones made xcvrd see 32 cages). Trigger not found (likely concurrent unserialised reads). Now: every access under one lock, a root-port health check and a dead marker that stops all access until reboot; recovery = power cycle (done 2026-11-16 17:00Z).
- Each RIF needs its own MAC on the loop. (The vtysh local-as/allowas-in lines are gone: local_asn, entry above.)

### 2026-11-16 — MACsec: engine capabilities, hitless rekey, MKA (hardware)

- Capability table of the Credo PHYs' EIP-163/164 engine from the vendor DDK (`libdriver_164_u`) plus the DWARF in Credo's 2026 `libSerdes_owl800`: all four GCM-AES / XPN suites, 4 ANs per receive SC, egress `SecY_SA_Chain` + `SecY_SA_Switch` (hitless), next-PN get/set, 64-bit counters, TCAM rules with EAPOL bypass. The vendor CLI uses a sliver of it: [macsec.md](macsec.md).
- Why yesterday's key change failed (`sa_set` edits memory only, `flush` = InsertEOP, second rule shadowed). Also: the vendor config sets `ROLLOVER` (PN wrap, I) and replay window 0xffffffff; `scripts/credo_macsec.py` no longer sets `ROLLOVER`. phy_server `system()`s unknown commands: its FIFO is now 0600.
- New `nix/nixos/module/cs8210/macsec/` (`hardware.cs8210.macsec`; imported and enabled on the machine in 251760c): phy_server shim (LD_PRELOAD, `cmx_*` commands, fail-closed gating, keys via 0600 files), `cs8210-macsec` tool (status, metrics, static keys, rekey, pn), `wpa_supplicant-cs8210` (`-D macsec_cs8210`), offline tests. vendor-sdk: `phyPreload` option.
- Hardware (cage 1 <-> 16 loop, ~35 min):
  - XPN-256 at 384.7 Gb/s per direction; hitless rekey with 0 loss (at 203 Gb/s and at line rate).
  - All four ciphers plus integrity-only interoperate with Linux kernel macsec.
  - Replay -> Late; wrong key -> NotValid; deleting the last SA fails closed.
  - MKA: secured in 3.3 s, rekey on natural PN exhaustion with 0 loss; wrong CAK stays closed; port flap recovers in 13 s.
  - Table + evidence: macsec.md "Hardware results", `analysis/macsec-hw-20261116/`.
- Bugs found on hardware, fixed in f5fd381 (stub DDK now models them), deployed with the SONiC-at-boot generation:
  - the `devices` pair layout;
  - `CfyE_Rule_Remove` leaves the TCAM entry live, which leaked **clear text** after down/up (now disable first);
  - counters are clear-on-read (now accumulated);
  - a live egress SA ignores a next-PN set;
  - the tool held its flock per object (deadlock), now per command.
- Box left with `cs8210-switch` stopped, drop-in removed, test keys/scripts deleted, cages 1 and 16 only.

### 2026-11-16 — SONiC: our own SAI ran the chip

- [sonic.md](sonic.md): no arm64 Teralynx SAI exists (98 packages checked), so `ai_re/nix/packages/sai-teralynx` = the AS7760's SAI frontend + an IFCS backend that dlopens the vendor libifcs.so from the SDK tree at run time (nothing vendor stored; `ifcs_api.h` = our declarations from the vendor ctypes, cited by sha256). Chip run: init, 32 ports, cage 1<->16 400G loop up in ~4 s, clean teardown (evidence ../sonic-nixos/cs8210-sai-hw-20261116.txt). Needed a Python-2.7 stub and an aligned memcpy for libifcs on PCI memory.
- `hardware.cs8210.sonic` imported, off (drv diff = flake source only); AS7760 toplevel drv unchanged (z05gsrr0...). Next: first full SONiC run (syncd/orchagent/FRR) by hand, VLAN-1 storm check on the loop first.

### 2026-11-15 — 400G + MACsec on the cage 1 <-> 16 DAC

- Module: one Mellanox CAB-Q400-P1M 1 m passive 400G-CR8 DAC between cages 1 and 16 (both MACsec ports).
- Credo line side `copper_1x400G` (line LT), runtime drop-in only: link up on every start, RS-544, 0 uncorrectable.
- ASIC-generated traffic (new `scripts/loop400_fwd.py` + `scripts/l2frames.py`): ~386 Gb/s each way, 0 errors, bypass and MACsec alike (the vendor 48 B IPG sets the rate). CPU path 2.8 Gb/s TCP; **fpp MTU 9462 drops TSO frames >= 9216 B** (use 9000).
- Static-key MACsec with our own keys (`scripts/credo_macsec.py`, vendor phy_shell SA commands, loaded live): ciphertext + SecTAG seen with one end in bypass; wrong key drops with InPktsNotValid. Rekey needs an SDK restart; MKA blocked on an SA install/delete path.
- New option `hardware.cs8210.vendorSdk.phyPostLoad` (not set on the box). Box left with cs8210-switch stopped, drop-in removed.
- Details and evidence: [macsec-400g.md](macsec-400g.md).

### 2026-11-15 — vendor-prompt batch: Route R works, stage3 keys, uC, CPLD live dump

- **Route R proven on hardware:** vendor U-Boot -> `loadx 0x01700000` route-r.bin (crc32 d5ef1e13) -> `go` -> ROM 3.1.0 UART CLI (`>`, boot_device UART CLI). ROM CLI `help` (12 commands), `rd`/`rdr` ok, ROM `xmodem 1800000` 64 KiB 0 retries, words as predicted; `quit` -> `Q`; then power cycle (plug 1 = CS8210 confirmed: /proc/uptime reset; `uptime` misleads, the RTC boots at Mar 17). Byte path: U-Boot `loadx` 64 KiB through consoled, 0 retries, crc32 671f0733. Fixed an argparse dest clash in `scripts/uart_recovery.py` (subcommand vs `--cmd`). Capture: `serial-20261115-vendor-batch.txt` (git-ignored).
- **Stage3 keys work on V3** when queued from stage2's banner: `p` -> `User request for skipping APCEA!` (sent after `Executing next` they arrive too late).
- Straps `0xfd8a8110` = 0x377feeef (bits 17:15 = 7 SPI mode 0, bit 18 = 1 debug off), `0xfd8a8278` bit 0 = 0; ROM alias at 0xffff0000 (crc32 d1a98647).
- uC: bootloader 2.28 / app 2.1.14, `uc_get_recovery` 0, SDR version 1, secure log counters 0.
- CPLD live dump (sys 254 regs, SWLED/MACLED 256): `analysis/cpld-diff/live-{dump,diff}-20261115.txt`. Differences vs the v13/v1-era expectations: sys 0x05 = 0x40 (fan force-max, normal without fand), sys 0x0d = 0xfa (one VRM status bit low vs 0xfb), SWLED/MACLED 0x01 scratch = 0xff (expected 0xde) and 0x09 = 0xff; 154 registers outside the known map read non-trivial values (mostly 0xcc). Analysis of what v15/v2 changed: open (cpld-diff.md).

### 2026-11-15 — node exporter

- `nixos/module/cs8210/default.nix` imports the shared `ai_re/nix/modules/monitoring/node-exporter.nix` (upstream node_exporter 1.12.1 on :9100, default collector set + systemd, textfile dir `/run/prometheus-textfile`); firewall opens 9100 on enp0s1 only (`openFirewall = false`).
- fand writes `fand.prom` there (`hardware.cs8210.fan.prometheusFile` now defaults to it; fand ordered after tmpfiles-resetup). Producer `cs8210` (every 15 s, no I2C) republishes fand's readings as `switch_board_info`, `switch_board_temperature_celsius{sensor}` (13 LM75s by position), `switch_asic_temperature{,_max}_celsius` for the switch-thermals dashboard; it fails when fand.prom is older than 30 s. `unmanagedMaxAge = 60` drops a stopped fand's file after a minute.
- Raw LM75s also come through node_exporter's hwmon collector (14 hwmon chips: 13 lm75b + one `800`).
- `nix store diff-closures` vs the running ias6p68f…: node_exporter, the producer/cleanup units, `source` (monorepo copy) only; unit diffs: fand (After, ReadWritePaths, config adds `runtime.prometheus_file`), firewall, tmpfiles, a `node-exporter` system user. Deploy restarts fand (brief failsafe to 100 %).

### 2026-11-15 — fand live; UART recovery mapped

- fand deployed (generation 26.11.20261114.2f709fe, also Linux 7.3.4; switch unit unchanged): `fand check --exec` all 14 sensors / 14 outputs ok, curve at PWM 127 after the 60 s first-reading failsafe; `systemctl stop fand` -> PWM 255, both watchdogs armed, sys CPLD force.
- [uart-recovery.md](uart-recovery.md): the ROM never reads console keys; UART XMODEM / UART CLI only via boot straps (PBS 0xfd8a8110 bits 17:15) or when no valid stage2 is found at any of 16 SPI offsets. A UART stage2 only repairs stage2 (TOC/stage3/U-Boot still come from SPI). Zero-write entry "Route R" (vendor U-Boot -> warm-reset CPU0 into the ROM CLI) designed and emulated, not run. Tools: `scripts/uart_recovery.py`, `scripts/rom_emu.py`.
- DMC-12 console byte loss (57%, this box's serial output) was the DMC-12's missing EHCI interrupt (IOFIC), fixed there; re-test 2.13 MB nonstop: 0 lost.

### 2026-11-15 — fan control moved to fand (deployed same day)

- `nix/nixos/module/cs8210/fan-control` now configures fand
  (`rust/crates/fand`, imported from the monorepo; the nepeat/nix flake is
  `git+file:…?dir=nix`, so `../rust` is in its source). Same curves and floor
  as fancontrol.py (board 40->55 C, ASIC 70->95 C, PWM 127 floor, 2 C
  hysteresis), plus: 14 outputs (one per fan slot, PWM 0x?2) with the fan
  watchdog 0x0c as `arm` (writes 0x18), CPLD version 0x00 = 3 as `expect`,
  front/rear tachs (x100, min 3000 rpm); the 13 LM75s by devicetree node (all
  required); the MCP3425 ASIC read as an exec sensor (`i2ctransfer r3@0x68`:
  an SMBus register read would write the ADC's config byte), critical 100 C;
  the system CPLD 0x05 bits 7/5 as an exec output (released every tick
  below 100 %, set at 100 %, RMW). ExecStopPost: `fand failsafe`, then
  `cs8210-fan-exit` (sys CPLD force, PWM 255, both watchdogs re-armed).
- The 0x74 mux is a kernel pca9548 (DT), not userspace-controlled: the manual
  `i2c mw 0x74 …` recipe is U-Boot only. Nothing on NixOS writes a mux by
  hand (tools, vendor-sdk scripts checked; the SDK chroot does not run the
  vendor install.sh, whose `-y 8` bus numbers are stock-kernel ones). The
  kernel locks the parent adapter per transfer, so fand, the lm75 driver and
  `cs8210-fan` cannot interleave. Mux channels are numbered in probe order,
  so fand got `bus: {of_node: "i2c-mux@74/i2c@2"}` (additive; tests in
  `src/i2c.rs`, `src/config.rs`) instead of DT `i2cN` aliases, which would
  renumber every bus and need a reboot.
- Checks: `cargo test -p fand` (and the aarch64 build's tests under
  emulation) pass; the rendered config passes `fand check --syntax-only` at
  build time; an offline run against a fake sysfs/CPLD tree (startup, SDK
  force-max, ASIC read failure, stalled rotor, watchdog re-armed, CPLD version
  change, SIGTERM, `fand failsafe`) behaved as intended. System built:
  `nix store diff-closures` vs master shows only the fan changes; the
  `cs8210-switch` unit, kernel, initrd and DTB are unchanged vs master.
- Open: tach scale x100 and the rotor maxima come from one fan sample; 7th
  slot populated on every board is assumed (`hardware.cs8210.fan.slots`).

### 2026-11-15 — boot chain dissected; boot ROM dumped; vendor U-Boot checklist

- [boot-chain.md](boot-chain.md): no signatures anywhere (stage2 unsigned, stage3/U-Boot byte sums only); recommended SPI scheme A' = second TOC at 0x20000 + BOOT_MODE + new app, factory TOC and UBOOT 0 untouched. General V3 notes: [../../alpine-v3-boot-chain.md](../../alpine-v3-boot-chain.md).
- [vendor-uboot.md](vendor-uboot.md): 27-item checklist for a replacement; new vs the DMC-12: **ASIC AVS** (Teralynx fuses -> 0.8 V on the regulator at I2C 0x62, VOUT 0x0333), SPI mux flip in do_bootm_states, external PCIe link-up; SerDes is stage3's job. Its read-only checks all matched on the box: [vendor-uboot-checks-20261115.txt](vendor-uboot-checks-20261115.txt).
- **Boot ROM dumped** at the vendor prompt: 0xfbff0000, 64 KiB, crc32 d1a98647 = U-Boot's `crc32`, sha256 7e1ec7ef55d1b851… (`analysis/boot-rom/`, git-ignored). Has `_stage2Load_loadToSramFromUart` / `xmodemReceive`. A single 64 KiB `md.l` lost ~57% of lines over the DMC-12 console (sustained output; consoled's ring too), so it was read in 1 KiB chunks, each checked against `crc32`.
- SRAM handoff words as predicted (`0xfbfe0000` = 0, `0xfbfe00f8` = 0x01100000). Stage3 `p` key test inconclusive: the keys (sent after `Executing next` via tmux) arrived after stage3's single poll; retry by sending from stage2's banner.

Older entries (2026-11-15 secure boot back to 2026-10-17, incl. the lettered 2026-11-10/11 entries (a)-(ab) other files cite as "PROGRESS (x)"): [HISTORY.md](HISTORY.md).

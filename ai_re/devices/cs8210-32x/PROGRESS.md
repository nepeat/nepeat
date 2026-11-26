# CS8210-32X

## Status (2026-11-26)

- **Runs:** NixOS 26.11 (2f709fe), Linux 7.3.4, generation system-54, from the SSD; a cold cycle of both feeds (`lab-power cs8210 cycle`,
  2026-11-26) came back unattended, 0 failed units; either PSU alone carries it under traffic. **SONiC at boot** on our C++ `sai-teralynx` (HEAD's frozen
  frontend, first on this box 2026-11-25). Cabling (owner, 2026-11-25; cage 1 and 2 modules replaced 2026-11-26): cages 1 <-> 2 and 31 <-> 32 Molex 1064281405
  400G-DR4 QSFP-DD optics (MPO loops), 3 <-> 17 TE 2459398-1 400G CR8 DAC; nothing to the house. CONFIG_DB = the Nix
  base: those six ports 1x400G RS FEC, far ends in Vrf-a/b/c with /31s, 26 ports down ([sonic.md](sonic.md) "Running it").
  **All three loops up at 400G, ping across + matching ASIC counters** (1 <-> 2 since 2026-11-26: cat fur on the fibre /
  optic faces; thin lanes: cage 2 Rx 1-2 -5.4 / -5.9 dBm, cage 32 Rx 1 -7.3 dBm) ([power-20261126.txt](evidence/power-20261126.txt)).
- **Cages:** `cs8210-optics` (`rust/cs8210`) is the one owner of the I2C FPGA: declared cages (= admin-up ports) out of
  reset and brought up (CMIS app select 4x100G -> 400G-DR4), all others held in reset; xcvrd and `show interfaces
  transceiver` through its socket; `switch_optic_*` metrics ([sonic.md](sonic.md) "Optics", [optics](evidence/optics-20261125.txt)).
- **LEDs:** system LED green (`cs8210-leds monitor`); port LEDs dark until the SAI sets up the ASIC LED stream (sonic.md
  "LEDs"); `cs8210-leds test|walk` for a human ([leds](evidence/leds-20261125.txt)). Also: fand (no failsafe phase at boot
  since 2026-11-26), PSU telemetry (`cs8210-psu`, `switch_psu_*`), Credo die temperatures, board RTC from the initrd,
  node exporter :9100 (job `cs8210-32x`), MACsec module (vendor stack only, [macsec.md](macsec.md)), sFlow.
- **Reach:** `sudo ssh root@10.60.2.190` (enp0s1). Console: DMC-12 P1 via consoled, `sudo ssh -t console@10.60.2.10 watch cs8210`
  (read-only) / `… cs8210` (attach, `^] w` takes write). **Power: two feeds**, PSU1 = NetBotz 250 outlet `cs8210-psu1`, PSU2 =
  lab plug 1; `direnv exec . lab-power cs8210 status|cycle` from ai_re/ (both; never target 2 = DMC-12), a cut only after
  `systemctl poweroff` printed `reboot: Power down`. The NetBotz outlet's normal state is On (`lab-power cs8210-psu1
  normal-state on|off`); PSUs in [board-control.md](board-control.md) "PSUs" (total draw = sum of both Pin, ~690 W idle).
- **Recover:** `systemctl stop sonic.target`; system-47 from the U-Boot menu (before cs8210-optics: its guard refuses syncd
  with these cages populated); dead I2C FPGA (`/run/cs8210-qsfp.dead`): power cycle. Vendor U-Boot prompt: `123<Enter>`
  within 3 s, then ONIE (`run onie_bootcmd`, RAM only) or **Route R** ([uart-recovery.md](uart-recovery.md), [boot-chain.md](boot-chain.md)).
- **Open:** port LEDs (SAI: the vendor `pci write led_*` init); our U-Boot items ([u-boot-port.md](u-boot-port.md)); publishing `ifcs_api.h` (owner); SAI gaps
  ([sai-features.md](sai-features.md)); MACsec under SONiC; CRM panels empty; learn-wedge watch (L0s off, [learn-wedge.md](learn-wedge.md)).

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
- A dead I2C FPGA stalls the whole SoC on every access (trigger unknown; PCIe remove/rescan not tried). Only
  `cs8210-optics` touches it (it holds `/run/lock/cs8210-qsfp.lock`, checks the root port, marks it dead); never map its
  BAR from another tool ([sai-hw-fpga-stall-20261116.txt](sai-hw-fpga-stall-20261116.txt)).
- **House network:** on 2026-11-11 a CPU 0 soft lockup on an SDK restart under load (round-2 ipd patches, since removed)
  knocked out the Unifi Aggregation <-> UDM link ((z)). The front ports are cabled only as the loops 1 <-> 2, 31 <-> 32,
  3 <-> 17 (cs8210-optics holds every other cage in reset, the guard refuses syncd if one holds a module); before
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
3. Credo slice temperatures and the ASC10 voltage/temperature monitors; optics as a fand input (owner: fan policy).
4. Populate `/var/lib/cs8210-sdk/root` reproducibly (from the `onie` MTD on the box) instead of by hand.
5. Port LEDs: the ASIC LED stream init in the SAI (sonic.md "LEDs"); a kernel driver for the MDIO FPGA.

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

### 2026-11-26 (c) — power: NetBotz normal state On, PSU telemetry without the kernel driver, redundancy, cold cycle

- `lab-power <netbotz> normal-state on|off` (`nboutlet -ns`, read back; fake-ssh tests); cs8210-psu1 set On.
- `cs8210-psu monitor` reads the PSUs' PMBus itself (PAGE its only write) and feeds `switch_psu_*` and sonic_platform:
  PSU1 Vout 12.06 V (the generic pmbus driver's probe had it at exponent 0, 6176 V), no more -1 rpm / vcap entries.
  Total draw = sum of both Pin (~349 + 343 W idle; one PSU alone ~701 W in).
- Redundancy under traffic (20 pps ping on all three loops): PSU1 off, then PSU2 off, each ~37 s: box up, the dead PSU
  NOT OK in `show platform psustatus` and logged by cs8210-psu, 0 % loss, 0 port errors; both restored.
- Clean poweroff + `lab-power cs8210 cycle`: back unattended (ssh ~64 s), 0 failed units, all six cages ready, three loops
  up, fand without failsafe. 1 <-> 2 came up at ~10:09 after the owner replaced both modules (cat fur on the faces).
- lab-power's `power` reading for cs8210 not wired: neither feed meters, and the PSU Pin lives on the switch (metrics).
  Evidence: [power-20261126.txt](evidence/power-20261126.txt).

### 2026-11-26 (b) — 1 <-> 2: lane 4 optical, not the Credo PHYs; page selects verified; xcvrd VDM settled

- Credo slices 1 / 2: host lanes ready, line lanes 12-19 not; already in optics mode (no line AN/LT), cage 3 copper.
  Both modules latch Rx CDR LOL + Rx power low warning on lane 4, continuously; cage 1's lane 4 Tx is 2 dB low.
  **Module host-side input loopback on cage 1 (13h:183 = ff): line lanes ready and Ethernet0 oper up**; cleared:
  down again. So ASIC, Credo (host LT, optics line mode) and module electrics are fine; the fault is optical lane 4,
  a human job (clean both MPO ends; if it stays, swap cage 1's module with 31's to see whether it follows). Breakout
  4 x 100G (app 2) would bring 3 of the 4 lanes up: not done ([links-20261126.txt](evidence/links-20261126.txt)).
- cs8210-optics reads every page select back: the Molex DR4s fall back to 00h on 2Fh, so a 2Fh read used to return
  page 00h's bytes and xcvrd's freeze write went nowhere. sonic_platform reports VDM only with page 2Fh: no freeze
  attempt and no "Failed to freeze VDM" since the deploy; the CDB query (9Fh exists) runs once per module (16 writes).
- Deployed live (switch, 808pa9pv): modules adopted, the other three links stayed up, 0 failed units. Rust 46 tests,
  clippy pedantic clean; sonic-platform tests pass.

### 2026-11-26 — platform round (CS-A2 B1): fan start-up, PSUs, board RTC, Credo temperatures, optics self-heal

- Deployed system-52 (sn9jxa55) = the working tree (nix-diff against the tree with HEAD's flake.lock: only the flake
  source path differs). Checks on the box ([platform-20261126.txt](evidence/platform-20261126.txt)):
  - fand starts at 60 %, the ASIC sensor "waiting" (startup grace 60 s) until its first reading 5 s later, then the
    curve (50 %): the ~65 s failsafe at 100 % on every boot is gone.
  - Board RTC (DS1339, `rtc-ds1307` in the initrd): the journal starts at the real time, no more March-2026 stamps.
  - PSUs (`cs8210-psu`, rust/cs8210): both Delta TDPS-1500AB-6 B fed, ~350 W in each, identity from PMBus, generic
    `pmbus` driver bound, `switch_psu_*`; sonic_platform has 2 bays and the readings. **PSU1 Vout reads 6176 V**
    (raw 0x1820 with exponent 0; PSU2 the same raw value at -9 = 12.08 V; VOUT_MODE 0x17 on both, reproducible on a
    plain rebind; PSU1 shows no page-1 status in debugfs): the driver's probe differs on PSU1, open.
  - Credo die temperatures `cs8210_credo_temperature_celsius{cage}` every 60 s via phy_shell (45-55 C).
  - xcvrd VDM: each optic fails one freeze (not confirmed in 1 s), sonic_platform then reports no VDM statistics for it:
    no repeats and no more module writes after 131 s. The CDB firmware query is asked once per module.
  - cs8210-optics: a failed bring-up retries every 60 s for ever; a module found out of Ready / Activated twice is
    brought up again; SONiC writes to the bytes the bring-up owns are refused. `credo-spec` picks the Credo line
    media per cage from the module (DAC copper, optics otherwise; cage 1 and 2 got `optics_1x400G`, 3 `copper`).
- Checks: cargo fmt / clippy (pedantic, -D warnings) / test green (rust/cs8210, 45 tests), `just test-packages -k
  cs8210-sonic-platform`, `just test-eval` (226 passed, 19 xfail).

### 2026-11-25 — new cabling; cs8210-optics (Rust) owns the I2C FPGA; three-loop config; system LED; reboot + cold cycle

- Optics read first (Python cs8210-qsfp, then the Rust owner, same bytes): 4 x Molex 1064281405 400G-DR4 QSFP-DD
  (CMIS 4.0, 12 W, default app 4 x 100G-FR1), 1 TE 2459398-1 1.8 m 400G CR8 DAC between 3 and 17
  ([optics-20261125.txt](evidence/optics-20261125.txt)).
- `rust/cs8210` (own workspace): `cs8210-optics` (register model + allow-list, stall safeguards, CMIS / SFF-8636
  bring-up, socket for sonic_platform, `switch_optic_*`), `cs8210-leds`; 38 tests, clippy pedantic clean, 6 unsafe
  blocks (README "Unsafe"). Python `cs8210-qsfp` and `sonic_platform/fpga.py` deleted; the guard asks the daemon.
- Config (machine `cs8210`): 6 ports up, Vrf-a/b/c far ends, Credo cage 3 copper, ASIC cage 17 copper; the saved
  config_db.json of the old loop moved aside. 3 <-> 17 and 31 <-> 32 up, 1000-ping per pair with matching ASIC
  counters, 0 errors; 1 <-> 2 down on Rx power (8-18 dB fibre loss; modules' own low alarm -8.9 dBm)
  ([links-20261125.txt](evidence/links-20261125.txt)).
- Deploy: switch-to-configuration boot + reboot (system-48), live switch (system-49, modules adopted, links stayed up),
  poweroff + plug 1 cold cycle: decode enabled, every cage from reset to ready in 21 s, syncd at 51 s, the same links.
  Deviation: the closure also carries HEAD's sai-teralynx (+291 KiB, the rust/sai commits' frozen C++ frontend) and a
  re-signed al_eth.ko (C source unchanged): not from this task.
- LEDs: port LEDs need the SAI (vendor `pci write led_*` lines, addresses in sonic.md "LEDs"); test colours / walk and
  the system LED proven by register read-back; what they look like only the owner can tell ([leds-20261125.txt](evidence/leds-20261125.txt)).
- Found: fand sits in failsafe (100 %) ~65 s on every boot (also on 11-16 and 11-18 boots); Credo die temperatures are
  not exported (only the LM75s beside the PHYs, in fand's board zone).

### 2026-11-18 — learn-wedge A/B done; powered off (plug 1)

- Crossover blocks 1-3 + forced L0s A/B: L0s causes the link errors (on 64-84 CE lines/h, off 0); stalls 2 on / 0 off,
  p = 0.25; production keeps L0s off ([learn-wedge.md](learn-wedge.md), `1a9bfb1`). 5 h after: clean.
- `systemctl poweroff` 02:49 PST: al_eth's shutdown path warns twice (`__flush_work` from `cancel_delayed_work_sync` in
  `al_eth_close`; refcount underflow in `phy_device_free`, i.e. a double put), then `reboot: Power down` does not cut power
  (hard LOCKUP 10 s later; same open PSCI item). Plug 1 off. Open: fix al_eth close/remove on shutdown.
  [shutdown-lockup-20261118.txt](shutdown-lockup-20261118.txt) (sha256 2595cd38aa3300a4…).

### 2026-11-17 — A' slot A rewritten with the fixed build; BOOT_MODE → slot A

- Same update path (plan 7676df81…; verify 133/133 from the installed model, sandbox rehearse): slot A (kill page over the old
  build, erase, body, header, magic last) + BOOT_MODE → 0x110, read back == model. Cold boot + warm reboot via slot A: all checks
  pass (FPGA BAR, Credo, BGP/BFD/LACP 2/2, AVS, fand, mtd0ro == model). Both slots now hold 58e4a4f0….
  [spi-upgrade-slotA-20261117.txt](spi-upgrade-slotA-20261117.txt).

### 2026-11-17 — A' update: slot B with the MDIO-FPGA BAR fix, BOOT_MODE flipped

- `spi_upgrade.py update --write` (plan 7364ea78…; verify 125/125 from the installed model, sandbox rehearse of the payload): slot B
  + BOOT_MODE (kill page, then body, header, magic last) read back == model. Reached the vendor prompt via `u` ("User request for loading U-Boot!").
- Cold boot and one warm reboot via slot B (stage3 loads 422752 B): MDIO FPGA BAR 0 assigned, Credo config done, BGP 2/2, BFD 2/2,
  LACP up, AVS active (ASIC COMMAND 0x0406, VOUT 0x0333), fand. [spi-upgrade-update-20261117.txt](spi-upgrade-update-20261117.txt).

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

### 2026-11-16/17 — learn-thread wedge: guard in libsai, ASPM L0s on the ASIC link, watchdog escalation (not deployed)

- sai-teralynx shim guards libifcs's learn-ring reads (GOT: node_pci_reg_read32/_write32, im_malloc): all-ones or
  out-of-range index re-read (3 ms apart), else the poll is skipped and the register not re-read while bad; im_malloc
  > 1 GiB refused; all-ones of any offset counted. Mock t_learn (1047 checks total) and chip (injected all-ones).
- First failure on 2026-11-16 was the CPU TX ring (ASIC stopped sending ~16:01; ring exactly full, 341 frames x 3
  descriptors, at 16:03:45 = the softirq spin; the bad read 2 min later). The ASIC link runs with ASPM L0s on
  (mainline's DT default, both U-Boots): replay timeouts every second, none with L0s off (A/B). Causation of the
  wedge: open, crossover soak running (on/off blocks >= 4 h).
- Watchdog: guard / cpu-tx signals, tx-stall witness, restart -> ipd reload -> alarm, lease/reload hold, root-port
  AER + LnkSta clear-and-count (dated), state across restarts; `cs8210-asic-aspm`; cs8210-switch MemoryMax 1536 MiB.
  Independent review: [reviews/2026-11-17-fable-learn-wedge.md](reviews/2026-11-17-fable-learn-wedge.md).
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

Older entries (2026-11-16 MACsec hardware and before): [HISTORY.md](HISTORY.md).

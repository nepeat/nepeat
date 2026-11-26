# CS8210-32X

## Status (2026-11-26)

- **Runs:** NixOS 26.11 (2f709fe), Linux 7.3.4, generation system-59, from the SSD; a cold cycle of both feeds (`lab-power cs8210 cycle`,
  2026-11-26) came back unattended, 0 failed units; either PSU alone carries it under traffic. **SONiC at boot** on our C++ `sai-teralynx` (HEAD's frozen
  frontend, first on this box 2026-11-25); its Rust replacement is at parity offline and passed H1 and H2 on the chip (H1: smoke + a short SONiC run on generation system-60; H2: 37 feature scripts Rust = C++ with SONiC stopped; box back on system-59 / C++; logs 2026-11-26 (h), (i)); H3 (SONiC under load on Rust) is next. Cabling (owner, 2026-11-25; cage 1 and 2 modules replaced 2026-11-26): cages 1 <-> 2 and 31 <-> 32 Molex 1064281405
  400G-DR4 QSFP-DD optics (MPO loops), 3 <-> 17 TE 2459398-1 400G CR8 DAC; nothing to the house. CONFIG_DB = the Nix
  base: those six ports 1x400G RS FEC, far ends in Vrf-a/b/c with /31s, 26 ports down ([sonic.md](sonic.md) "Running it").
  **3 <-> 17 and 31 <-> 32 up; 1 <-> 2 was up 10:09-10:27 (cat fur on the fibre / optic faces, both modules replaced)
  and is down again after the reboots: cage 1 Rx lane 3 -7.4 dBm + CDR loss of lock** ([reboots-20261126.txt](evidence/reboots-20261126.txt)).
- **Cages:** `cs8210-optics` (`rust/cs8210`) is the one owner of the I2C FPGA: declared cages (= admin-up ports) out of
  reset and brought up (CMIS app select 4x100G -> 400G-DR4), all others held in reset; xcvrd and `show interfaces
  transceiver` through its socket; `switch_optic_*` metrics ([sonic.md](sonic.md) "Optics", [optics](evidence/optics-20261125.txt)).
- **LEDs:** system LED green (`cs8210-leds monitor`); port LEDs: LED 1 of 4 magenta on every linked cage from libifcs' own LED
  stream (it sets it up at node create, V), health on LEDs 2-4 proposed (I; sonic.md "LEDs"); fan tray LEDs by fand; `cs8210-leds test|walk` for a human ([leds](evidence/leds-20261125.txt)). Also: fand (no failsafe phase at boot
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
- **Open:** **three fan modules with a dead rotor** (FAN5 rear, FAN6 front, FAN12 rear; LED solid red, still following the
  zone duty): 3 failed rotors <= 4, fand degraded on its curve (~52 %, ASIC ~63 C); replace them, then `fand release`
  the slot if it was swapped while off ([board-control.md](board-control.md) "Fan CPLD register map"); `lab-power cs8210 cycle` once failed with
  NetBotz "E101: Command Not Found" (not reproduced; lab-power now resends once on E101); the 1 <-> 2 fibre into cage 1; port LEDs 2-4 (slot codes and colours unknown, I; all LED registers proven R/W at H2 and restored, V; the panel effect needs the owner's replay of [led-panel-checklist.md](led-panel-checklist.md); H1 read LED_CFG = 0x805: libifcs' LEARN_MSG-based read-modify-write left no junk in bits 4-10, V); Rust SAI: R1 (IFCS key padding) found at H2 and fixed (byte twins, ABI fixture, byte-level parity tap); H3 next (soak and reboots on Rust, the watchdog's guard signal on it, the default switch); our U-Boot items ([u-boot-port.md](u-boot-port.md)); publishing `ifcs_api.h` (owner); SAI gaps
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

**Fans** (user request 2026-11-09: keep them quiet). Chassis labels: FAN1..7 = bottom slots 1..7, FAN8..14 = top slots
1..7 (silkscreen, confirmed by pulls). fand holds PWM 127 (~50 %, ~17k rpm) until the hottest LM75 passes 40 C or the ASIC
die 70 C, full speed at 55 / 95 C. A stalled rotor first gets a short temperature-guarded "calm restart" (all fans low
together, then up); one that stays dead is given up (solid red, its module keeps the zone duty, remembered across
reboots); the zones stay on their curve (degraded) and only sensor loss, critical temperature or more than 4 failed
rotors force full speed. Tray LEDs: green ok, blinking red =
fand handling it, solid red = failed or absent ([board-control.md](board-control.md) "Fan CPLD register map").
A stop forces full speed and re-arms the fan watchdog.
`fand status`, `fand set 80% --for 10m`; `cs8210-fan status|set|quiet|max|watchdog` when no daemon runs;
`hardware.cs8210.fan.controller = "python"` is the old fallback. Code: `nix/nixos/module/cs8210/{fan-control,tools}`.
Tach x150 (vendor driver and fans.yaml). **Still loud from power-on until NixOS userspace (~40 s):** the
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
5. Port LEDs 2-4: the owner replays the register walk at the panel ([led-panel-checklist.md](led-panel-checklist.md); all LED registers are R/W, H2, sonic.md "LEDs"); a kernel driver for the MDIO FPGA.

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

### 2026-11-26 (i) — H2: the feature matrix on the chip with the Rust SAI

- 37 adapted oracle scripts ([scripts/h2/](scripts/h2/); loops 31 &lt;-&gt; 32 primary, 3 &lt;-&gt; 17 variants) run on the box with SONiC stopped, Rust libsai vs the C++ libsai, the same Rust `sai-script` and `tl-frames` on both: statuses, values, frame results, diag lines and libifcs error lines compared per step ([04-feature-scripts.txt](evidence/rust-h2/04-feature-scripts.txt)).
- First pass: 17 PASS, 5 feature-PASS with a cleanup leak, 14 FAIL, 1 inconclusive. One root cause, **R1**: the Rust rim passed by-value IFCS key structs (route / FDB keys) with uninitialised padding, libifcs compares raw key bytes, so route / FDB delete and replace answered NOTFOUND on the chip (some SAI statuses 0 = leaked entries, others -7 / -17, next hops / RIFs / VLANs then BUSY). The mock compares fields, so offline parity missed it ([06-ifcs-key-padding.txt](evidence/rust-h2/06-ifcs-key-padding.txt)).
- Fix: byte-twin types in `ifcs-sys`, an ABI proof fixture (x86_64 + aarch64), a byte-level tap in the parity harness with a poison negative control; 0 unsafe blocks added ([12-offline-proof.txt](evidence/rust-h2/12-offline-proof.txt)). Re-run of the 20 affected scripts: **all 20 PASS**, so all 37 PASS; no route / FDB NOTFOUND, BUSY or -17 in 60 re-runs (30 Rust, 30 C++) ([13-feature-rerun.txt](evidence/rust-h2/13-feature-rerun.txt)). Deliberate non-differences: the /32 count with the prefix table full varies run to run on both libsais (hash placement); counters differ by a few frames of timing.
- Learn-ring guard injection (all-ones reads of 0x4860 / 0x4864 / 0x485c) on the Rust shim = the C shim: 15 journal lines byte-identical, `.prom` counters identical, VmSize flat ([03-guard-inject.txt](evidence/rust-h2/03-guard-inject.txt)). The watchdog's guard signal under SONiC on Rust is not re-proven (H3).
- LED registers (Rust libsai post-init list, 92 steps, owner not at the panel): all R/W; per-PIC words and the blink timer persist across libifcs deinit/init (only LED_CFG and the refresh registers are re-programmed at init); a blink time base exists (off after init); everything restored to R1 ([01-led-walk.txt](evidence/rust-h2/01-led-walk.txt)). What the LEDs show: pending, owner replay [led-panel-checklist.md](led-panel-checklist.md).
- Cage 32 (DR4, marginal lane 1): link missing or late in 13 of the 60 re-runs on either libsai; the verdicts use a pair where both sides had link. 3 &lt;-&gt; 17 had link in every run. [sai-features.md](sai-features.md) Rust column: `hw H2` rows.

### 2026-11-26 (h) — H1: the Rust SAI on the chip

- SONiC stopped, Credo alone: Rust libsai `sai-teralynx-smoke --sonic-is-stopped` SMOKE_OK (3<->17, 31<->32, `--recreate`), guard 1/1/1; IFCS trace equal to the C++ libsai's call for call (only live temperatures differ) ([rust-h1/12-trace-diff.txt](evidence/rust-h1/12-trace-diff.txt)).
- LED registers read (R1): LED_CFG 0x805, refresh 0x15e / 0x18eec2 enabled, blink timer off, SHIFT/COLOR masks 0xffffff, SWCTRL 0 on all 33 PICs, constant over link and admin changes; the predicted LED_CFG junk bits from libifcs' LEARN_MSG-based RMW are NOT present on the chip ([13-led-r1-table.txt](evidence/rust-h1/13-led-r1-table.txt)).
- `hardware.cs8210.sonic.saiImplementation` ("cpp" default; with FLAKE pinned the default toplevel drv is unchanged): system-60 = SONiC on the Rust libsai, ports up, ASIC_DB as on C++ (909 keys), then back to system-59 (C++). 31<->32 sometimes needs >60 s to link after an SDK restart (cage 32 lane 1).
- Open: the drop-in's tools need libstdc++ for libifcs outside syncd (fixed in the package, see rust/sai README); `sai_api_query` answers -5 vs the C++ -15 for API ids beyond the table (a listed deliberate difference); the guard's one all-ones read of 0x104 is seen only on C++ inits (counted only).

### 2026-11-26 (g) — SAI in Rust: the Teralynx backend at parity with the C++ on the mock (offline, not on the box)

- `rust/sai` has the CS8210's libsai in Rust (`sai-backend-teralynx` over the `ifcs` rim, `ifcs-sys` bindings
  generated at build, `elf-got`) and its shim (`ifcs-shim` + `ifcs-guard`): 44467af (scaffold), eb9a34c (every area).
- On the mock libifcs: sai-teralynx's `test_ifcs` 1047 / 1047 on the Rust libsai; parity harness strict green (IFCS
  calls of all 17 sections equal, driver scripts 376 / 376 ops, oracle scripts 25 / 27 equal + 2 allowed: the runner's
  genetlink name clash). The Rust shim is a drop-in for the C one (416-script differential, x86_64 + aarch64
  emulated; one deliberate fix: the guard's counters buffer). Per feature: [sai-features.md](sai-features.md) "Rust" column.
- Rust only: a board register list after every init and a read-only register watch (diag `regs`), for the LEDs.
  [sonic.md](sonic.md) "LEDs" corrected: libifcs sets up the LED stream itself; a linked cage lights LED 1.
- G5: IFCS header generator in Rust (`rust/sai/crates/ifcs-gen`), byte-equal to `gen_ifcs_api.py` on ishell 2.0.1
  (headers sha256 6353998884fc... / c7c75f397eca...), checked live by `rust/sai/nix/ifcs-headers.nix` until the C++
  package is deleted; nothing changes on the box.
- Nothing deployed: the box runs the C++ SAI. Next on the box: a smoke with SONiC stopped, then the hw:* scripts and
  SONiC on Rust. Details: [rust/sai README](../../../rust/sai/README.md) "Teralynx backend", "Parity".

### 2026-11-26 (f) — fans: given-up rotors keep running, state across reboots

- Owner policy: a module with one dead rotor is not stopped (half a module of airflow); fand `stall_recovery.given_up:
  follow` (CS8210 only; other boards keep `isolate`), no duty-0 isolation, given-up rotors persisted in
  /var/lib/fand/recovery.json (migrated from /run once). Live switch to system-59: zones from FAILSAFE back to degraded
  (3 failed rotors), LEDs solid red on FAN5 / FAN6 / FAN12; 5 min after: duty 52 %, ASIC 62.6 C, board zone 40.4 C.
  Caveat (subagent, owner's call): in a true failsafe the dead rotors are driven at 100 % again.
- Two of the three rotor losses came with a reboot; trace and a gentler start/stop proposal in board-control.md
  "Fans at reboot" (not built).

### 2026-11-26 (e) — B4: 5 reboots, 35-min line-rate soak on the two up loops

- 5 reboots (fand degraded policy): 0 failed units, no fand failsafe line, ASIC online in 5 s, 6/6 cages ready every time;
  3 <-> 17 + 31 <-> 32 up in 4 of 5 (31 <-> 32 lost lock once on cage 32's thin lane 1); 1 <-> 2 down throughout.
- A third fan module stalled at boot (FAN6 front) and no dip revives it: 3 isolated modules -> FAILSAFE 100 % (policy).
- Soak 35 min, SONIC_RING_LOOPS=2 (sonic-ring.sh now 3 or 2 loops): 386 / 367 Gb/s per direction, 0 port errors / discards,
  0 uncorrectable FEC (cage 32 lane 1 at -7.5 dBm gives 12e9 corrected codewords, the other ports 0.8-32e6), ASIC <= 55.9 C,
  modules <= 39 C, Credo <= 57 C, syncd RSS flat, watchdog / learn-ring guard 0; ring torn down, CONFIG_DB identical
  ([soak-20261126.txt](evidence/soak-20261126.txt)). First try stopped at once on fand reporting degraded as failsafe
  (metric fixed, system-58).

### 2026-11-26 (d) — fans: FAN12 rear rotor stall, a burnt-smell scare (false alarm), fan CPLD map

- Top fan 5 (FAN12) rear rotor read 0 rpm from the 10:38 boot; fand failsafe 100 %. ~11:10 the owner cut power for a burnt
  smell; it was a false alarm (warm electronics), both feeds back on, NetBotz normal state On.
- Hot-pulled and reinserted fan modules stay unpowered; fan CPLD map from the vendor driver in board-control.md; work open.

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

Older entries (2026-11-17 scheme A' install, update and MDIO FPGA BAR entries and before): [HISTORY.md](HISTORY.md).

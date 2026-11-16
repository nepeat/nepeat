# CS8210-32X

## identity

- device model: Celestica CS8210-32X (user identification; Celestica CS8210
  confirmed by the SSD's device tree and diagnostic package). Codename: Briggs.
- chassis S/N: `C8210F2B02211113T0029` (provided label photo).
- SSD: Western Digital Ultrastar DC SA210, 120 GB M.2 SATA.
- SSD model: `HBS3A1912A4M4B1`; S/N: `A069BCC5`; P/N: `0TS1653`.
- SSD firmware: `M6102100`; exact capacity: 120,034,123,776 bytes.
- acquisition attachment: USB adapter, `/dev/sdb`; serial confirmed against photo.
- platform software: ARM64, Annapurna Labs Alpine; Innovium/Teralynx SDK.
- serial console: **115200 8N1, no flow control — confirmed 2026-11-09** on the
  console port via FTDI FT232R adapter; clean text from the first stage-2 loader
  byte through U-Boot and ONIE. Pinout and levels were not measured by the agent.
- tmux session `serial-cs8210`; power via `tools/lab_power.py` (HA lab plug 1: `lab-power 1 …`).
- CPU: 16x Cortex-A72 (`0x41/0xd08`), 8 GB DDR (2 x 4 GB, SPD at 0x52), Alpine v3.
- boot chain: AL stage2/3 v3.65.1 -> U-Boot 2018.05-alpine_db-v3.65.1.015 ->
  ONIE 2017.11.015 (FIT from SPI `0x410000`), kernel 4.9.124+.
- EEPROM: CS8210-32X-UN-00, P/N R3181-F9003-01, rev R0B-2, mfg 2021-04-28 (TH),
  base MAC 0C:48:C6:7A:55:CE (4 MACs), BoardSN R3181-G0001-02TH0321110078.
- CPLDs: System 15, SWLED 2, MACLED 1, top/bottom fan 3.
- board components: not physically inspected by the agent; see storage notes in the
  2026-11-09 log entry.

## todo / standing notes

- **In flight (2026-11-15):** node exporter built, not deployed yet (log entry "node exporter"), then the ai-re-metrics target; 400G + MACsec on the cage 1 <-> 16 loop done by hand, see [macsec-400g.md](macsec-400g.md) (only cages 1 and 16 are populated, re-check before every SDK start); MACsec key changing + MKA implemented and tested offline (2026-11-16, [macsec.md](macsec.md)), hardware window pending; SONiC on NixOS with a Teralynx SAI (agent, offline first); U-Boot replacement (scheme A' in boot-chain.md, no write planned yet); vendor-prompt batch (UART route R, uC readouts, CPLD v15/v2 register dump, cpld-diff.md); switch-porting-runbook.md refresh from the AS7760.

- **External PCIe INTx (2026-11-15, from the DMC-12 USB finding):** on the DMC-12 (Alpine V2) the factory DT routes external-PCIe INTx through an Annapurna IOFIC that mainline lacks, so its USB only ran from the EHCI 100 ms watchdog. Here the ASIC (0001:01:00.0, `ipd`) has legacy irq 17 = GIC SPI 73, today only PME/AER on it (SDK not running); the FPGAs have no driver. With `cs8210-switch` running (2026-11-15) ipd uses 8 MSI-X vectors (napi and pic fire, legacy irq 17 unused), so INTx does not matter for the ASIC. On-SoC devices are fine: AHCI and ttyS0 count interrupts, al_eth uses MSI-X.

- **Fans** (user request 2026-11-09: slow them down, the unit is very loud at the
  CPLD power-on default; keep them quiet). Linux holds PWM 127 (~50%, ~11k rpm)
  until the hottest LM75 passes 40 C or the ASIC die 70 C, full speed at 55 /
  95 C; any fault or stop forces full speed and re-arms the fan watchdog.
  Daemon: **fand** (deployed 2026-11-15; stop test: PWM 255 + watchdogs armed):
  `hardware.cs8210.fan.controller` = `fand` (default) | `python` (fallback).
  Manual control: `fand status`, `fand set 80% --for 10m`; `cs8210-fan
  status|set|quiet|max|watchdog` for when no daemon runs.
  **Still loud between power-on and NixOS userspace (~40 s)**: nothing in
  U-Boot touches the fans (no env var, no script, only the CPLD version
  banner). The U-Boot equivalent would be raw writes, not yet tried:
  `i2c dev 3; i2c mw 0x74 0.0 0x04` (top; `0x08` = bottom), then
  `i2c mw 0x66 0x0c 0x18` and `i2c mw 0x66 0x22 0x7f` ... `0x82`.
- Fan control scripts and hardware convenience scripts are in the nix tree
  (user request 2026-11-09): `nixos/module/cs8210/{fan-control,tools}`.
- Tach scale is an assumption: `cs8210-fan` prints raw x 100 rpm.

## current status

- **Console (2026-11-15):** DMC-12 port P1 via consoled: `sudo ssh -t console@10.60.2.10 watch cs8210` (read-only) / `… cs8210` (attach, `^] w` takes write); `ssh console@10.60.2.10 log cs8210` prints the scrollback. Up 4 d 20 h at the time, NixOS, Linux 7.3.3.

**NixOS 26.11 / mainline Linux 7.3.3 on the SSD, booting unattended.**
Working: serial console, 16 cores, 8 GiB, SATA SSD, management Ethernet
(al_eth + 88E1510), TPM 2.0, lldpd, I2C sensors, RTC, fan control, all three
external PCIe ports. **Switch side (started by hand, `systemctl start
cs8210-switch`): Teralynx 7 ASIC initialised by the vendor SDK running in a
chroot against Marvell's open-source `ipd` driver (legacy-ABI patch), 32
front-panel netdevs `fpp1`..`fpp32`, Credo MACsec PHYs on ports 1-16 with
firmware loaded, in MACsec bypass.**
Fans temperature-controlled including the ASIC die (about 55% at idle).
**First link 2026-11-10: cage 17 (QSA + 10G DAC) up at 10G, LLDP received from the neighbour (entry (p)).**
**400G 2026-11-15: cage 1 <-> 16 DAC at ~386 Gb/s per direction through both Credo PHYs, also with static-key MACsec (GCM-AES-128/256), by hand ([macsec-400g.md](macsec-400g.md)).**
Not working / unproven: MKA, hitless rekey and MACsec at boot on hardware (implemented, offline tests pass, 2026-11-16: [macsec.md](macsec.md)), Linux offload, aux SPI flash, second NIC.
Config: `nepeat/nix`, flake output `cs8210`. The vendor userland lives only on
the switch (`/var/lib/cs8210-sdk/root`). ONIE in SPI flash is intact and is
the fallback boot; the factory DIAGOS exists only as the 2026-10-17 image.

Earlier offline work: [ssd-analysis.md](ssd-analysis.md),
[kernel-review.md](kernel-review.md), [offline research](offline-research.md),
[board-control.md](board-control.md), [serial candidate](bringup/README.md),
[Fable review triage](reviews/2026-10-17-triage.md).

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

### 2026-11-16 — MACsec: engine capabilities, hitless rekey, MKA (offline)

- Capability table of the Credo PHYs' EIP-163/164 engine from the vendor DDK (`libdriver_164_u`) plus the DWARF in Credo's 2026 `libSerdes_owl800`: all four GCM-AES / XPN suites, 4 ANs per receive SC, egress `SecY_SA_Chain` + `SecY_SA_Switch` (hitless), next-PN get/set, 64-bit counters, TCAM rules with EAPOL bypass. The vendor CLI uses a sliver of it: [macsec.md](macsec.md).
- Why yesterday's key change failed (`sa_set` edits memory only, `flush` = InsertEOP, second rule shadowed). Also: the vendor config sets `ROLLOVER` (PN wrap, I) and replay window 0xffffffff; `scripts/credo_macsec.py` no longer sets `ROLLOVER`. phy_server `system()`s unknown commands: its FIFO is now 0600.
- New `nix/nixos/module/cs8210/macsec/` (`hardware.cs8210.macsec`, off, **not imported yet**): phy_server shim (LD_PRELOAD, `cmx_*` commands, fail-closed gating, keys via 0600 files), `cs8210-macsec` tool (status, metrics, static keys, rekey, pn), `wpa_supplicant-cs8210` (`-D macsec_cs8210`), offline tests. vendor-sdk: `phyPreload` option.
- Offline: `test/run-test.sh` and `test/run-mka-test.sh` PASS (MKA PSK bring-up, PN-exhaustion rekey via Chain + Switch, fail closed on exit). aarch64 builds OK. Hardware: not run (plan in macsec.md).

### 2026-11-16 — SONiC: our own SAI ran the chip

- [sonic.md](sonic.md): no arm64 Teralynx SAI exists (98 packages checked), so `ai_re/nix/packages/sai-teralynx` = the AS7760's SAI frontend + an IFCS backend that dlopens the vendor libifcs.so from the SDK tree at run time (nothing vendor stored; `ifcs_api.h` = our declarations from the vendor ctypes, cited by sha256). Chip run: init, 32 ports, cage 1<->16 400G loop up in ~4 s, clean teardown (evidence ../sonic-nixos/cs8210-sai-hw-20261116.txt). Needed a Python-2.7 stub and an aligned memcpy for libifcs on PCI memory.
- `hardware.cs8210.sonic` imported, off (drv diff = flake source only); AS7760 toplevel drv unchanged (z05gsrr0...). Next: first full SONiC run (syncd/orchagent/FRR) by hand, VLAN-1 storm check on the loop first.

### 2026-11-15 — 400G + MACsec on the cage 1 <-> 16 DAC

- Module: one Mellanox CAB-Q400-P1M 1 m passive 400G-CR8 DAC between cages 1 and 16 (both MACsec ports).
- Credo line side `copper_1x400G` (line LT), runtime drop-in only: link up on every start, RS-544, 0 uncorrectable.
- ASIC-generated traffic (new `scripts/loop400_fwd.py` + `scripts/l2frames.py`): ~386 Gb/s each way, 0 errors, bypass and MACsec alike (the vendor 48 B IPG sets the rate). CPU path 2.8 Gb/s TCP; **fpp MTU 9462 drops TSO frames >= 9216 B** (use 9000).
- Static-key MACsec with our own keys (`scripts/credo_macsec.py`, vendor phy_shell SA commands, loaded live): ciphertext + SecTAG seen with one end in bypass; wrong key drops with InPktsNotValid. Rekey needs an SDK restart; MKA blocked on an SA install/delete path.
- New option `hardware.cs8210.vendorSdk.phyPostLoad` (not deployed). Box left with cs8210-switch stopped, drop-in removed.
- Details and evidence: [macsec-400g.md](macsec-400g.md).

### 2026-11-15 — vendor-prompt batch: Route R works, stage3 keys, uC, CPLD live dump

- **Route R proven on hardware:** vendor U-Boot -> `loadx 0x01700000` route-r.bin (crc32 d5ef1e13) -> `go` -> ROM 3.1.0 UART CLI (`>`, boot_device UART CLI). ROM CLI `help` (12 commands), `rd`/`rdr` ok, ROM `xmodem 1800000` 64 KiB 0 retries, words as predicted; `quit` -> `Q`; then power cycle (plug 1 = CS8210 confirmed: /proc/uptime reset; `uptime` misleads, the RTC boots at Mar 17). Byte path: U-Boot `loadx` 64 KiB through consoled, 0 retries, crc32 671f0733. Fixed an argparse dest clash in `scripts/uart_recovery.py` (subcommand vs `--cmd`). Capture: `serial-20261115-vendor-batch.txt` (git-ignored).
- **Stage3 keys work on V3** when queued from stage2's banner: `p` -> `User request for skipping APCEA!` (sent after `Executing next` they arrive too late).
- Straps `0xfd8a8110` = 0x377feeef (bits 17:15 = 7 SPI mode 0, bit 18 = 1 debug off), `0xfd8a8278` bit 0 = 0; ROM alias at 0xffff0000 (crc32 d1a98647).
- uC: bootloader 2.28 / app 2.1.14, `uc_get_recovery` 0, SDR version 1, secure log counters 0.
- CPLD live dump (sys 254 regs, SWLED/MACLED 256): `analysis/cpld-diff/live-{dump,diff}-20261115.txt`. Differences vs the v13/v1-era expectations: sys 0x05 = 0x40 (fan force-max, normal without fand), sys 0x0d = 0xfa (one VRM status bit low vs 0xfb), SWLED/MACLED 0x01 scratch = 0xff (expected 0xde) and 0x09 = 0xff; 154 registers outside the known map read non-trivial values (mostly 0xcc). Analysis of what v15/v2 changed: open (cpld-diff.md).

### 2026-11-15 — node exporter (built, not deployed)

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

### 2026-11-15 — secure boot is OFF (OTP read at the vendor prompt)

- Rebooted to `ALPINE_DB>` (`123⏎`, 20 s after `systemctl reboot`), typed only read-only commands, booted NixOS again (up in 65 s). Evidence: [otp-readout-20261115.txt](otp-readout-20261115.txt) (sha256 733a3b54b497…).
- `otp show direct` = `shadow`: **secure boot disabled, all 4 PUK hashes zero and unlocked**, boot from SPI/UART/NAND enabled, JTAG/SPI debug enabled, min SW version 0, TZ off. `uc_get_secure`: NOT-SECURE (uC app 2.1.14). So stage2/3 should accept unsigned objects, as on the DMC-12; stage2's "Command is not supported in secure-mode" is something narrower (open, boot-chain agent).
- `flash_contents_toc_print` (mx25u25635f, 32 MiB): STG2 0x0, DT 0x41000, SerDes FW 0x50000/0x5c000, SBus master FW 0x68000, STG3 0x70000 (max 0x120000), UBOOT 0x190000 (max 0x1f0000), UBOOT_ENV 0x380000, UBOOT_ENV_RED 0x3c0000 (Linux calls it mtd2 "unused"). **No BOOT_MODE object and no spare app slots** (DMC-12 had IPXE 0/1).
- `dev_info`: SPI(M0) boot, 4x4 cores, CPU 2000 / DDR 1200 MHz.
- **Danger:** vendor `otp` has password-gated *irreversible* subcommands (secure-boot-enable, puk-hash-set, …). Only ever `otp show direct|shadow`.

### 2026-11-11 (ab) — offline "mini-switchd" design + dry-run generator (Fable), and why the ASIC can/can't loop

Offline only; the switch is unplugged. Everything is in [switchd/](switchd/):
[DESIGN.md](switchd/DESIGN.md) plus the stdlib tool `switchd_dryrun.py`,
which turns desired-state JSON into the ordered SDK commands for apply and
teardown, validated offline against the vendor CLI's own parser
definitions and `ifcs_ctypes`.
- **Default forwarding (V from vendor source, spot-checked by me):**
  - **Steady knet state cannot loop.** `cmds/knet.py` gives each front
    port its own L2VNI (vlan = i+1) and flood group, with members CPU +
    that port only.
  - **The vendor's `-s` single-L2-domain mode would loop** (one L2VNI for
    all ports). We never pass it.
  - **Init window: unknown default.** `auto_load_user.sh` (~L214-228)
    initialises the ASIC twice: a `python2.7 ifcsshell.py` pass, then
    `innovium.user`. Ports are enabled before `diagtest knet config` runs,
    so for those moments the forwarding state is the SDK's built-in default.
  - **Teardown leaves links up:** `cls_shell exit` is `kill -9` (cls_shell
    L29), so a dead SDK leaves links up with no LACP behind them. That fits
    the (z) incident; pause frames from a stuck CPU queue are an
    alternative explanation.
- **Proposed guard (not implemented yet):** the vendor `:down-pall` option
  on every port spec entry, so all ports start admin-down; after init
  `port disable`, an isolation check (`diagtest knet show`), the desired
  state, and the `port enable` allow-list last. On stop, `port disable`
  before `cls_shell exit`. A crash or soft-lockup still needs a
  data-plane power-off/reset path (open).
- **LAG / L2 mapping:** the CLI has no create verbs, so objects are created
  in Python 2 inside innovium.user (`cls_shell 'run <module>'`). LAG
  (`ifcs_lag_create`, `ifcs_lag_member_add`) needs members detached from
  their knet L2VNI/mc_dest first. A bridge is STP + mdg + l2vni + members +
  PVID `ifcs set sysport …`, with static `l2_entry` for LACP/LLDP group MACs
  to the CPU. **knet uses VLANs 2..36**, so user VLANs must avoid them; the
  tool refuses collisions. ipd has no switchdev/offload_fwd_mark, so a Linux
  bridge on hardware-bridged ports would double-forward floods (a loop).
  Bridges should live only in the ASIC until ipd is patched.
- **Validation:** the port mapper matches the vendor dynamic_config.yaml for
  all 128 devports. The example-03 plan is 27 CLI + 41 API steps, 0
  invalid. The self-test rejects 13 deliberately wrong commands; I re-ran it
  and regenerated example 03 myself (identical). Not validated: libifcs
  semantics and return codes (especially LAG), `run <module>` via the remote
  shell, a real Python 2 run, hardware behaviour. DESIGN.md §6 lists 15
  unknowns.

### 2026-11-11 (aa) — FIB capacity from the SDK (offline, Sonnet swarm): 24,576 v4 prefixes on this build

There is no public number. Four parallel Sonnet agents worked offline on
the local SDK: the usage API, the forward profiles, the HW table catalogue,
and defaults/hints. Summary in [fib-capacity.md](fib-capacity.md).
- 7 forward profiles A-G with constant tables in libifcs (matching the
  pyctypes constants, V). With ILPM the chip would do up to 294,912 v4 /
  147,456 v6 prefixes (profile C).
- **This SDK build has no ILPM** (all `ii_ilpm_*` symbols weak-undefined,
  checked with nm, V), so prefixes go only into the TCAM: **24,576 IPv4 /
  12,288 IPv6**. The live boot log shows `IMT3008B_CAM_CTRL depth: 24576`
  (V). The default profile is A (reset writes 0, V): 98,304 v4 hosts,
  48,128 v6 hosts, 45,056 MACs.
- The profile can only be chosen at node creation (YAML `forward_profile`).
  The CLI `usage` reports the TCAM count. The bench fill test (management
  only, no front-panel cables) is in fib-capacity.md.

### 2026-11-11 (z) — INCIDENT: round-2 ipd patches → CPU 0 soft lockup on SDK restart under load; house network knocked out

**Round-2 patches (Fable 5.2):** ipd-tx-flow-control-napi (TX stop/wake
flow control, bounded reaping, TX completions on a 9th MSI-X vector + own
NAPI on CPU 1), ipd-rx-page-pool-refill (page_pool, one refill doorbell
per poll, discard leak fixes), ipd-tx-sg-gso-optin (off by default).
Deployed with an ipd reload; vector 8 fired on CPU 1 (2.86M interrupts).
Measured over the cage-2 2x10G bond (V):

| test | round 1 | round 2 |
|---|---|---|
| into switch, 1 stream | 1.69 Gb/s | 2.22 Gb/s |
| out of switch, 1 stream | 1.07 Gb/s | 2.33 Gb/s |
| out, -P4 | 1.5 Gb/s | 2.35 Gb/s |
| bidir -P4 60 s | ~2.2 Gb/s total, TX bimodal | 1.03 in + 1.58 out = 2.6 Gb/s |

TX-3 Full 0, qdisc requeues 0, no dmesg warnings.

**Then:** `systemctl restart cs8210-switch` during a bidir iperf (Fable's
suggested lifecycle test) → **soft lockup on CPU 0 in `PORT_BRUP` (SDK
thread, i.e. inside an ipd ioctl), "stuck for 399 s"**, RCU stalls.
systemd-networkd was "blocked on a mutex likely owned by PORT_BRUP", so
the thread spun **holding rtnl** (ipd's INNO_ND_STATE/LINKSTATE ioctls take
rtnl around dev_open/dev_close). Management became unreachable, journald and
kworkers hung in I/O wait, the serial shell stopped executing, and SysRq
'b' is disabled in this kernel ("This sysrq operation is disabled"). No
stack trace was captured: the console showed only the lockup/RCU headers,
and `dmesg > /root/lockup-20261111.txt` never completed. Cause not proven.
The desktop side of the test (started 01:04 PST) finished only at 03:21
(V): the ssh session running `systemctl restart cs8210-switch` never
printed `is-active` and died with "Connection timed out", so the restart
never returned. The 150 s ping probe of the bond peer timed out (its
"reachable 152 s" line is the timeout, not a reply). Over the 60 s iperf
window the client logged 1.76 GB out / 2.74 GB in (290 retransmits), i.e.
~1.4 / ~2.2 Gb/s during the ~10 s before the restart and nothing after.
The data path stopped at the restart.
The likely area is ring free/re-init while TX/RX NAPI is still running
(Fable already listed "ring free races with a running NAPI" as
pre-existing; its new tx_reap_lock / second NAPI / page_pool teardown add
to that path).

**Network impact:** while the box was hung, the user's network broke: the
**USW Pro Aggregation ↔ UDM Pro link went down**. Hypothesis (not
measured): with the ASIC half re-initialised and the CPU dead, LACP stopped,
the Unifi treated cage-2 legs 1 and 2 as individual ports, and the
Teralynx switched frames between them in hardware → loop / broadcast storm
(or UniFi loop protection / STP blocking the uplink). Earlier, unaggregated
legs already showed heavy flooding (lane 2 ~30k frames/s, enp0s1 ~5k
pkt/s). After the user approved it, the lab plug was cycled; the user then
hard-powered the switch off and **unplugged it**.

**Actions:** round-2 patches removed from `ipdPatches` (files kept, comment
in kernel.nix). The build is back to round 1 (GRO/NAPI) + the earlier fixes.
cs8210-switch is not enabled at boot, so booting the box does not start
the data plane.

**Before the data plane goes near the house network again:**
- find the PORT_BRUP lockup (reproduce on the bench with only management
  connected and a kernel with sysrq + softlockup_panic/backtraces, or
  kdump);
- learn what the ASIC forwards by default during SDK bring-up/teardown
  (VLAN 1 membership? flooding between fpp ports?) and put a guard in place,
  e.g. one leg only, or the ports kept admin-down until knet trap rules are
  in;
- prefer a cable topology that cannot loop the house network (a separate
  test switch/VLAN).

**Note:** the script that first added this entry truncated PROGRESS.md
(`open(p,'w').write(open(p).read())`), and commit 1d813f4 deleted the
whole file. Restored from 475c800 and this entry re-added in the next
commit.

### 2026-11-11 (y) — robustness: wedge watchdog, deploys no longer reset the chip, phy_server busy-loop fixed

- **phy_server at 100% of a core (V, fixed):** strace showed ~29k
  `read(3, "", 1024) = 0` per second. fd 3 is `/tmp/cls_phy_fifo`
  (O_RDONLY). Once the last writer closes, a FIFO read returns EOF
  immediately, so the daemon's command loop never blocks. Fix in
  `cs8210-switch-up`: after the Credo loader, a `sleep 2147483647` holds a
  writer open on the FIFO, so read() blocks. phy_shell still works. The
  first try used `exec -a <name> sleep`, and the chroot's sleep is a
  busybox symlink, which picks the applet from argv[0] and exited at once.
  Result: phy_server 0 CPU ticks in 5 s (was 500), sleeping in
  anon_pipe_read.
- **Deploys reset the chip (fixed):** the cs8210-switch unit's `path`
  included `config.system.path`, so any package change altered the unit
  and `switch-to-configuration` restarted it. Now `path = [ chroot switchUp
  ]`, and the wrappers get a fixed binPath (procps, sed, coreutils,
  ethtool, iproute2, systemd, util-linux, bash) plus
  `/run/current-system/sw/bin` appended at runtime (cs8210-show etc.). The
  unit's PATH has no system-path any more (V).
- **Learn-thread wedge watchdog (V):** `cs8210-sdk-watchdog` checks every
  60 s for a kernel line matching `comm: ifcs-learn` since cs8210-switch's
  ActiveEnterTimestamp, restarts cs8210-switch, and does at most one
  restart per 15 min. End-to-end test: a clearly marked fake line was
  written to /dev/kmsg at 00:55:35 (it says "WATCHDOG TEST (fake, injected
  by hand)", so ignore it in later wedge analysis). The watchdog restarted
  the SDK at 00:56:27, the SDK was up at 00:57:49, and bond0 answered ~80 s
  later (usually ~30 s; maybe the Unifi re-admitting LAG members after
  back-to-back flaps). This is a stop-gap; the real fix is still the
  all-ones read shim from the 2026-11-10 crash review.

### 2026-11-11 (x) — first traffic through a Credo PHY: cage 2 4x10G LAG in MACsec bypass (double_CRC was the bug)

The user moved the breakout to cage 2. Presence: bit 1 only (matches the
pin map). fpp2-1/2 came up at 10G full through the Credo PHY. The PHY
reports host and line PCS up on slice 2 ports 0/1, and the generated
`c-phy/dynamic_config.txt` has `firmware_configure_port … speed=10000
mode=MacsecBypass line_fec=NO_FEC host_fec=NO_FEC`. But:
- ASIC devport 2 received **100% bad frames** (rx_frames_err_crc =
  rx_frames_all, 320/324, then 335/339, at the normal LLDP/LACP rate and
  sizes). The PCS error counters stayed frozen at their link-up values, so
  the lanes were clean. Linux saw 0 frames, so there was no LACP partner
  and no lease.
- The loader writes `double_CRC slice_id=S port=P enable=1` for every
  port, also with `-e macsecbypass`. **`phy_shell "double_CRC slice_id=2
  port=0|1 enable=0"` fixed it at once (V):** the CRC errors froze at 374,
  the next 35 frames were OK, and LACP partnered with the Unifi on both
  legs.
- **Fix in `cs8210-switch-up`:** in bypass mode, after the Credo loader,
  turn double_CRC off for every `double_CRC … enable=1` line in
  dynamic_config.txt (failures logged, not fatal). `sync_bond_macs` now
  reconfigures every bond with fpp members, not only when a MAC was fixed,
  because DHCP can fail during the corrupt-frame window too.
- **Unattended check (V):** deploy → SDK restart → bond reachable 28 s after
  the SDK was up; devport 2 1,757/1,757 frames OK; LACP 2 ports, partner the
  Unifi; lease 10.60.30.163.
- **Throughput through the Credo path (V):** iperf3 -P4: into the switch
  1.61 Gb/s, out 1.55 Gb/s. Both legs carry traffic (devport 2 and 3), 0
  CRC errors. That is the same as cage 17 without a PHY, so the CPU path is
  still the limit.
- For MACsec proper: double CRC may be required there (MACsec framing on
  the host side); revisit when the QSFP↔QSFP loop cable arrives. The flag
  0x0 vs 0x4 in the loader's port log is still not decoded.

### 2026-11-11 (w) — soak + LACP failover results; LAG moved to cage 2 (Credo PHY, 4x10G, MACsec bypass)

**Soak (stopped by the user at 27.5 min):** iperf3 `--bidir -P4` from the
desktop over bond0 (2 legs, fpp17-1/2, LACP with the Unifi), with the bond
watched over enp0s1.
- Switch RX 267 GB (avg **1.39 Gb/s**) and switch TX 164 GB (avg 854 Mb/s)
  at the same time. 0 iperf errors, ipd RX ring drops unchanged (8), 0 new
  ifcs/ipd/AER kernel lines, no learn-thread wedge. TX-3 "full" reached
  146k (backpressure: ipd returns TX_BUSY without stopping the queue).
- **Bottleneck (V):** `ksoftirqd/0` at ~99%. All ipd work (one MSI vector,
  one NAPI) runs on CPU 0, and RX starves TX there. RPS on fpp17-* (rps_cpus
  fffe, live only, not in nix) spread the stack work (10% softirq overall),
  but CPU 0 stayed pegged. Switch TX is **bimodal**: phases of ~200 Mb/s and
  ~1.6 Gb/s, both before and after RPS, so RPS did not clearly fix it. Next:
  batched refill, page recycling, more RX vectors (Fable's list).
- `phy_server` (Credo) uses ~95% of one core continuously (busy poll).
  Unrelated to the cage-17 path; worth a look.
- Hashing is uneven: leg 1 sent 17.1M packets, leg 2 5.7M.
- **LACP failover (V), under load:**

| event | time | result |
|---|---|---|
| pull leg 1 | 00:17:51 | removed from aggregator, bond0 stayed UP, 0 iperf errors |
| replug leg 1 | 00:20:44.7 → 00:20:45.8 | back in the aggregator in ~1 s (fast LACP) |
| pull leg 2 | ~00:21:05 | same: bond0 up on leg 1, 0 errors |
| replug leg 2 | ~00:21:44 | rejoined, 10G full, bond MAC kept |

**Cage 2 LAG (Credo port, MACsec bypass):** the user is moving the breakout
from cage 17 to cage 2. New option `hardware.cs8210.vendorSdk.credoPorts`
(env `CS8210_CREDO_PORTS`) overrides the c-phy spec the way innoviumPorts
does for the ASIC. Machine config:
- ASIC `1:copper_1x400G;2:copper_4x10G;3-16:copper_1x400G;17-32:optics_1x400G`
- Credo `1:optics_1x400G;2:copper_4x10G;3-32:optics_1x400G` (`-e macsecbypass`)
- bond0 members `fpp2 fpp2-*`; cage 17 is back to 1x400G (`fpp17`).

After deploy: 35 eth devports, `fpp2-1..4` enslaved on the bond MAC. The
Credo log shows slice 2 with 4 ports at 10000, 1 lane each, host FEC 0,
config flag 0x0 (the 400G ports show 0x4; meaning not decoded). No link
yet: the cable is not moved.

### 2026-11-10 (v) — bond survives SDK restarts unattended (member-MAC race fixed)

Test #1 from the list (SDK restart regression) failed first, as expected,
and is fixed now.
- **Cause (V, `ip -ts monitor link` across a restart):** the SDK creates
  `inno_0_0017` with a random MAC and renames it to `fpp17-1`. networkd
  immediately pins the bond MAC and enslaves it (23:39:39.870), and ~50 ms
  later (39.919) the SDK sets its own per-port MAC …:df on the netdev,
  overwriting the bond MAC on an enslaved member. Replies to the bond MAC
  are then PACKET_OTHERHOST and dropped. ipd itself only sets the MAC at
  creation, before register_netdev, so the overwrite comes from SDK
  userspace.
- Second effect: bond0's DHCP ran during that window, lost its replies and
  sat in retry backoff (no lease 2+ minutes later). A `networkctl
  reconfigure bond0` gets a lease within seconds.
- **Fix (`cs8210-switch-up` `sync_bond_macs`):** after auto_load_user.sh
  returns and the speeds are set, every fpp* that is a bonding member and
  does not carry its master's MAC is set to it, then `networkctl
  reconfigure` runs for each bond that needed it. The networkd member pin
  stays (harmless) with a corrected comment. The wrapper PATH gains
  iproute2 and systemd.
- **Result (V):** a plain `systemctl restart cs8210-switch` gives bond0
  reachable from the desktop 28 s after the SDK is up, lease 10.60.30.163,
  fpp17-1/2 on 0e:48:c6:7a:55:ce, LACP partner the Unifi, no
  intervention. A few "failed to get link speed/duplex" kernel lines per
  member before `set_speeds` runs are expected.

### 2026-11-10 (u) — LACP narrowed to cage 17 (SFP28 breakout), VRF, I2C on the breakout, RX 4x (Fable patch)

- The user meant LACP on **cage 17 only**: bond0 members are now `fpp17
  fpp17-*`. enp0s1 is back to plain DHCP on its own MAC (10.60.2.190). The
  other fpp* are unmanaged again.
- **VRF:** bond0 is in `vrf-fp` (table 100) so the front-panel path has its
  own default route, and replies don't go out via enp0s1.
  `net.ipv4.{tcp,udp}_l3mdev_accept=1` lets sshd and iperf3 in the default
  VRF answer on bond0. Clients: `ip vrf exec vrf-fp …`.
- **Cable:** the user swapped to an FS QSFP28→4×SFP28 breakout, with legs 1
  and 2 to the Unifi. **I2C (V):** present bit 16 only, and `cs8210-qsfp
  --info 17` decodes SFF-8636: id 0x11 QSFP28, PN `QSFP-4SFP25-CU3M` rev 5,
  date 2026-06-27, page 0. Only the QSFP head's EEPROM is reachable from
  our side; each SFP28 leg's EEPROM is read by the Unifi. Legs: fpp17-1
  and fpp17-2 LOWER_UP, 17-3/4 down (not connected).
- **LACP with the Unifi (V):** Partner MAC 78:45:58:bd:a4:29 (the Unifi), so
  LACPDUs reach the CPU and the bond negotiates.
- **Still broken: member MAC.** After an SDK/ipd restart the fpp17-*
  members keep their own fpp MACs (…:df/…:e0) instead of the bond MAC, and
  replies to the bond MAC are then dropped as PACKET_OTHERHOST. The
  networkd `[Link] MACAddress=` pin in the member .network did *not* fix
  it. Workaround used: `ip link set fpp17-N address 0e:48:c6:7a:55:ce` by
  hand. Proper fix TBD (find who resets it: SDK ioctl vs bonding; or set it
  in cs8210-switch-up after the SDK is up).
- Unifi side: lane 2 received ~30k frames/s and enp0s1 ~5k pkt/s of
  flooding while the legs weren't aggregated. ASIC TX on both lanes stayed
  tiny (490 / 135 frames total), so the switch is not looping anything.
- **RX throughput (Fable 5.2 patch `ipd-rx-gro-napi.patch`):**
  `napi_gro_receive` instead of `netif_receive_skb`, NAPI weight 8 → 64,
  and RXQ CIDX MMIO re-read once per batch instead of once per packet.
  Deployed with an ipd reload. **desktop → switch: 1.69 Gb/s single
  stream** (was 396-429 Mb/s), 1.64 Gb/s with -P4; TX 996 Mb/s. RX-0 ring:
  2.94M packets, 0 drops. No "No vlan tag action" messages. Known side
  effect: `rx_dropped` is no longer counted on the GRO path.
- The management port briefly could not ARP its gateway with a static
  address while networkd was stopped. It got 10.60.2.190 by DHCP once
  networkd ran with the new config. Not investigated further.

### 2026-11-10 (t) — LACP bond over enp0s1 + all front-panel ports (networkd); three blockers fixed

User request: LACP over every front-panel port and the management port.
- **networkd instead of scripted networking/dhcpcd:** fpp* exist only while
  the SDK runs and are re-created on every restart, and networkd enslaves
  them whenever they appear. `bond0`: 802.3ad, lacp_rate fast,
  ad_select=bandwidth, layer3+4 hashing, miimon 100 ms; members `enp0s1
  fpp*`. DHCP and RA on bond0, DHCP client id = MAC. inno_* and enp0s3
  stay unmanaged. wait-online is disabled. The dhcpcd LogLevelMax override
  is now conditional on dhcpcd being enabled.
- **Blocker 1, ASIC MAC filtering (V):** with a random bond MAC, DHCP
  (broadcast) worked but ARP replies and ping did not. The ASIC traps
  unicast to the CPU only for MACs the SDK programmed: the base MAC
  `0e:48:c6:7a:55:ce` (ONIE MAC#1Base with first byte + 2) and the per-port
  fpp MACs. Both the base MAC and fpp17-1's MAC tested OK, so bond0 is
  pinned to the base MAC. The bond got 10.60.30.179 again on Unifi port 7's
  VLAN.
- **Blocker 2, al_eth reopen bug (V, fixed in tree):** enp0s1 could not be
  opened again after a close with no cable (`bad speed parameter (-1)`,
  `al_eth_hw_init failed to configure link parameters`), which also
  happens with a plain `ip link set up`, no bond needed. phylib leaves
  SPEED_UNKNOWN and the HAL rejects it. `al_eth_hw_init` now programs a
  1G/full placeholder when the speed is unknown, and adjust_link reprograms
  the MAC on link-up. The "link config" message went from err to dbg.
  After an `al_eth` reload, enp0s1 enslaves fine (cable still unplugged).
- **Blocker 3, no ethtool on ipd (V, fixed):** bonding read no
  duplex/speed, so 802.3ad disabled LACP on every fpp (actor key 0) and
  ad_select=bandwidth would rate them 0 Mb/s. New
  `ipd-ethtool-link-ksettings.patch`: get reports full duplex plus a speed
  stored by `ethtool -s <dev> speed N duplex full autoneg off`; set raises
  NETDEV_CHANGE. `cs8210-switch-up` sets every fpp's speed from the same
  port spec the ASIC got. Result: fpp17-1 Speed 10000 / Full, actor key 15,
  port state 0x4f (active, fast, aggregatable, in sync, *defaulted*).
- **Still open:** Partner MAC 00:00:… means no LACPDU from the Unifi yet.
  Port 7 is not an aggregate on the Unifi side, so the bond runs it as an
  individual link. Whether LACPDUs (01:80:c2:00:00:02) reach the CPU is
  untested until the Unifi speaks LACP. The bond's 802.3ad system MAC still
  shows the random a6:78:… captured at bond creation (cosmetic; it changes
  when the bond is re-created).
- Side effect: every activation that changes `system.path` restarts
  cs8210-switch (its unit PATH includes the system path), so deploys reset
  the chip. Worth narrowing.

### 2026-11-10 (s) — CPU TX stall root-caused to the ipd TX ring, fixed with a doorbell barrier

**Symptom ((r)):** CPU TX on fpp17-1 stops for good under load. Reproducible
ramp (`iperf3 -R -b`, 5 s steps, switch TX): 20M / 100M / 300M fine
(Linux tx_packets = devport-17 tx_frames ±2), and **600M stalls**. A port
bounce and a physical cable cycle do not clear it; an SDK restart (ring
re-init) does.

**Where (V, read on the stalled box over serial):**
- `/sys/devices/ipd/ring_stat`: TX-3 (the netdev TX ring) Full =
  23,305,720 after the first stall; Bypass packets frozen.
- Debug log line from `inno_enet_tx` (loglevel 6 for 50 ms): `TX ring 3
  pidx: 0x258 cidx: 0xcc(0xcc)`.
- Hardware registers via the vendor CLI `pci read`: `TXQ_3_DESC_CIDX` =
  **0xcc**, `TXQ_3_DESC_PIDX` = 0x2a0, `TXQ_3_DESC_RING` = 0x400,
  `TXQ_3` = 0x30001, `TXQ_3_CIDX_UPDATE_TMR_CTL` = 0x2. So the ASIC's TX
  DMA engine itself stopped consuming at descriptor 0xcc. It is not a
  missed CIDX writeback, and the ASIC egress path is not involved.
- With the ring stuck, skbs never complete, so a sender blocks on its
  socket send buffer (ping -c300 hung in sendmsg) before the ring fills.
- The sysfs `resource0` mmap of the ASIC BAR returns EINVAL (ipd holds the
  region), so BAR reads have to go through the SDK CLI (`pci read <NAME>`).

**Cause (I, strongly supported):** `inno_enet_tx` writes 3 descriptors per
packet (LDH header, data, FCS/pad) into the ring with memcpy, then rings
the doorbell with a raw volatile store `REG32(TXQ_x_DESC_PIDX) = pidx`,
with no barrier. That is fine on x86. On arm64, nothing orders Normal-memory
stores before a Device-memory store, so the ASIC can fetch a half-written
descriptor and the queue halts. The RX refill doorbells have the same gap:
their `smp_mb()` (dmb ish) sits before the descriptor writes.

**Fix:** `nix/.../kernel/patches/ipd-arm64-doorbell-wmb.patch` puts `wmb()`
before the TX doorbell and both RX refill doorbells, which is what
`writel()` would do. It was deployed without a reboot: SDK stopped,
`rmmod ipd`, then `MODULE_DIR=/run/current-system/kernel-modules/lib/modules
modprobe ipd` (srcversion C3E17271… → BB200611…, legacy_abi=Y), SDK
started.

**After the fix (V):**
| test | result |
|---|---|
| ramp -R 300M / 600M / 1G / unlimited, 10 s each | 300 / 600 / 995 / 1050 Mb/s, Linux TX = port-17 TX ±3, ping OK after each |
| -R unlimited 60 s | 7.45 GB, **1.07 Gb/s**, no stall |
| -R -P4 20 s | **1.50 Gb/s** sum |
| forward (switch RX) 30 s | 396 Mb/s (RX ceiling, separate issue) |
| TX-3 ring after all of it | 11,258,539 packets, 16.5 GB, Full 62 (backpressure, recovered) |

No A/B rerun of the old module after the fix, so "missing barrier" is the
explanation that fits, not a proven mechanism. The stall was 100%
reproducible at 600M before, and 11M packets at up to 1.5 Gb/s since show
none. The temporary iptables rule for 5201 has been removed again. The
fpp17-1 address is now static (10.60.30.179/24, the same DHCP lease), set
by hand; the dhcpcd service is running again for enp0s1.

### 2026-11-10 (r) — iperf3 over fpp17-1: RX 429 Mb/s, then the CPU→port TX path stalls

iperf3 3.21 (aarch64, same nixpkgs pin) was copied over fpp17-1, with
port 5201 opened by a temporary `nixos-fw` rule on fpp17-1. The desktop
reaches the switch routed through 10.60.2.1.
- **desktop → switch (switch RX): 429 Mb/s** over 10 s, 65 retransmits.
- **switch → desktop (`-R`, switch TX): hung.** After that the switch did
  not answer on fpp17-1 at all (ping 100% lost both ways, ssh gone). The
  link stayed up and there was nothing new in dmesg. A server socket sat in
  FIN-WAIT-1 with 11 bytes unsent.
- Where the frames stop, from counters read over serial:
  - Linux: fpp17-1 `tx_packets` counts every new ping, and the fq_codel
    qdisc shows 0 drops and 0 requeues, so `inno_enet_tx` accepts frames
    and the ipd TX ring is not full (a full ring returns NETDEV_TX_BUSY
    and would show requeues).
  - ASIC CPU port (devport 0) `rx_frames_all` keeps rising (408,398 →
    410,228). **Corrected in (s):** this counter does not count CPU→ASIC
    frames (it rose ~19k while Linux sent 185k), so it says nothing about
    the TX ring.
  - devport 17 `tx_frames_all` is frozen at 230,208. Of the 370,941 large
    frames (1519-2047 B, i.e. MTU frames plus the CPU header) that the CPU
    port received, devport 17 sent only 42,990 frames of 1024-1518 B.
  - (Superseded by (s): the TX DMA ring itself stops.) So frames are dropped or held inside the ASIC between CPU-port ingress
    (pipeline bypass, `syshdr` PIPELINE_BYPASS to sysport 17) and devport-17
    egress. devport 17 shows no pause/PFC frames and no drop counters (none
    are non-zero).
- `port disable 17` / `port enable 17` in cls_shell did not clear it.
  The bounce also removed the fpp17-1 routes (the address stayed); the
  routes were re-added by hand and ping still failed. Most likely an SDK
  restart is needed (not done yet).
- One `cls_shell "ifcs show devport 0 stats"` call hung and was Ctrl-C'd.
  innovium.user was idle and not wedged.
- Correction: the `transns 2` in `ip -s -s link` is the carrier-transition
  count, not TX watchdog timeouts. ipd's `ndo_tx_timeout` only bumps
  `tx_errors`, and that stayed 0.
- Hypothesis (unverified): the CPU-injected bypass traffic outran
  devport 17's 10G egress, and a buffer or credit pool for CPU-sourced
  traffic did not recover. Next: an SDK counter or buffer dump for the
  CPU port / sysport 17 queues before restarting, then rate-limited TX
  (`iperf3 -R -b`) to find the threshold.

### 2026-11-10 (q) — management over the front panel: DHCP, ping, ssh via fpp17-1

At ~22:18 the user unplugged the management port (enp0s1 link down), and
fpp17-1 was configured from the serial console.
- `dhcpcd -4 -1 fpp17-1` at first only sent the request to the running
  dhcpcd service, which ignores fpp*. After `systemctl stop dhcpcd`, a
  standalone one-shot instance leased **10.60.30.179/24** from 10.60.30.1,
  with a default route via 10.60.30.1 (V). So the CPU path carries
  broadcast and unicast both ways, not only LLDP.
- Ping from the switch: gateway 10.60.30.1 0/3 lost, Unifi 10.60.30.52 0/3
  lost, 1.1.1.1 0/3 lost (~11 ms). ARP resolved for both neighbours.
- From sea420-desktop (10.60.2.x, routed): ping 0/3 lost (0.19 ms), and
  **ssh root@10.60.30.179 works** (V). fpp17-1 counters at that point: TX
  64 packets / 0 errors, RX 19,762 (19,047 dropped by the kernel: LAN
  broadcast and multicast).
- The state is hand-made and does not survive a reboot or an SDK restart.
  The dhcpcd service is stopped, and the one-shot lease (24 h) is not
  renewed. To go back: `systemctl start dhcpcd` once enp0s1 is plugged in
  again. To make it permanent, add fpp17-1 to the NixOS network config;
  but the netdev exists only while cs8210-switch runs, and the SDK learn
  thread still wedges every 5-10 h, so keep the serial console or enp0s1
  as the fallback.

### 2026-11-10 (p) — first link: cage 17 at 10G to the USW Pro Aggregation, LLDP both ways

The far end of the cage-17 DAC is SFP+ port 7 of a Unifi USW Pro Aggregation.
The user plugged it in at about 22:00, after the 21:49 wedge, so module
insertion did not trigger that wedge.
- New option `hardware.cs8210.vendorSdk.innoviumPorts` (env
  `CS8210_INNV_PORTS` for `cs8210-switch-up`). It overrides the uniform
  ASIC port spec, written in the vendor `dynamic_config.sh` syntax. The
  machine config sets
  `1-16:copper_1x400G:fec-def;17:copper_4x10G:fec-def;18-32:optics_1x400G:fec-def`.
  Before deploying, a dry run of `dynamic_config.sh` in the chroot gave 35
  eth devports. Cage 17 becomes devports 17-20 (ISG15 lanes 0-3, 10G, FEC
  NONE, the vendor's NRZ 10G DAC preemphasis table), and cage 18 moves to
  devport 21. The knet netdevs are named by cage: `fpp17-1`..`fpp17-4`,
  while `fpp18`..`fpp32` keep their names.
- Deployed with `switch` (no reboot), which also clears the wedged SDK:
  cs8210-switch restarted, chip reset, bring-up ~1 min. The same deploy
  carries the six Fable-review fixes and the SFF-8472 decoder.
- **`fpp17-1` UP / LOWER_UP right after bring-up (V).** `ifcs show devport
  17`: speed 10G, link_status UP, oper_state LINK_UP, pcs_status 1, local
  and remote fault 0, FEC NONE, no AN or link training.
- **LLDP (V):** lldpd on fpp17-1 sees chassis `USWProAggregation`
  (USW-Pro-Aggregation 7.5.15), port "Port 7" / "SFP_ 7". That is RX
  through the ASIC, the ipd CPU path and knet. TX counter: 6 packets (our
  LLDP frames). Far end (UniFi app, user screenshots): "Disconnected" at
  22:15, "10 GbE" at 22:16 with 2.7 MB up / 1 KB down. Nothing was changed
  in between, so this was UI polling lag. Its 1 KB down matches the ASIC's
  `tx_bytes_ok` of 1,608 (V: TX reaches the far end). PCS counters
  (block_err 8051, ber 102, sync_header 106) did not move across samples
  20 s apart, so they are left over from link-up. RX: 3,902 packets, of which 3,761 were dropped by the kernel (LAN
  broadcast/multicast with no host config on the netdev); 0 errors.

### 2026-11-10 (o) — first module: QSA + FS 10G DAC in cage 17 reads through the FPGA

The user fitted a QSA in port 17 holding an FS 10G SFP+ passive DAC (3 m).
- Presence: BAR0 `0x140` = `0x00010000` (bit 16 only), with presence-change
  bit 16 set. Front-panel 17 = channel 16 = i2c-117, which agrees with
  the FPGA-derived pin map (V, one port).
- Released ResetL on 17 (`--reset 17 0`), then `cs8210-qsfp --info 17`.
  256 bytes of SFF-8472 A0h read back (eight 32-byte transactions). A second
  read was byte-identical, and both checksums are correct. Decode: SFP+
  passive cable, 10300 Mb/s, 3 m, vendor "OEM" (FS's generic coding), PN
  `SFP-H10GB-CU3M`, date 2026-02-07, no DDM. 0x51 ACKs and reads `0xff`.
- This is the first data transfer through the I2C FPGA on mainline, and no
  console errors appeared. Details:
  [fpga/i2cfpga/hw/README.md](fpga/i2cfpga/hw/README.md) row 5. The cable's
  serial number is masked in the committed capture.
- `cs8210-qsfp --info` printed a wrong "byte 127 != 0, re-run with --page 0"
  note for SFPs, and it had no SFF-8472 decode. Both fixed in the nix tree
  (tested offline on the captured bytes; not deployed yet).
- **SDK learn thread wedged a third time** at 21:49:51 PST, 5.6 h after the
  16:11 restart. Same signature: 32 GiB `ifcs-learn` allocation, then
  "Learn WB w/o expected SOP", with ifcs-learn spinning at ~99% of one core.
  The containment from (n) held: /run at 1%, SDK tmpfs at 14%, logtrim
  dropping ~22 MB/s. Root-port AER bits are the same as the earlier capture,
  so they don't date it. The "timer callback took longer" lags (up to 3.5 s)
  also occur well before the wedge, so they are not a precursor. Whether the
  module insertion was near 21:49 is not known; the FPGA keeps no timestamp.
  Evidence: `analysis/research/ifcs-log-20261110/live/wedge3-2149.txt` (local).
- Not done yet: link. The far end of the DAC and the SDK port config (port
  17 is a 400G port behind no Credo PHY; a QSA needs it set to 1x10G) are
  the next step.

### 2026-11-10 (n) — first live I2C FPGA tests; SDK learn-thread wedge; /run and SSD fixes

**I2C FPGA on hardware, empty cages** ([fpga/i2cfpga/hw/README.md](fpga/i2cfpga/hw/README.md)).
These are the first transactions through the FPGA on mainline, with
`cs8210-qsfp` plus a timing script, allowlisted BAR0 accesses only, and no
errors on the serial console.
- Power-up `cb+0x00` = `0x00081766` on all 32 channels, as layer 7
  predicted. `0x100c` reads 0 after the probe writes `0x80000000`, so bit 31
  self-clears (V). That broke `cs8210-qsfp`'s readiness check; fixed and
  deployed.
- A 1-byte read at 0x50 on each of the 32 ports returns status
  `0x00000002` (NACK), with busy clear, 32/32 (V). The crosstalk check is
  weak, because `0x60` is not writable in the allowlist.
- Busy time on port 1 for sel 0/1/2: 36.7 / 19.6 / 125.3 us (median of 300).
  Least squares on the layer-7 bit model 6(code+1)+13 clocks gives 11.8
  bit-times + 5.8 us overhead, with residuals <= 0.03 us. For sel 0 that is
  30.9 us of bus time against the simulation's 30.5 us (entry (m)), which
  supports the SCL rates of 386/868/99 kHz (I: SCL was not probed).

**SDK learn-thread wedge** ([reviews/2026-11-10-sdk-learn-crash.md](reviews/2026-11-10-sdk-learn-crash.md)).
- innovium.user wedged twice this boot: 02:36:31 (44 min after start) and
  11:05:10 (3 h 44 min after start). Both times the kernel logged
  `ifcs-learn` asking for 34359726080 bytes, then the SDK flooded
  `Learn WB w/o expected SOP` at ~25 MB/s into its /tmp under /run.
- That flood filled the 1.9 GB /run tmpfs (RAM): logind failed with ENOSPC
  from 02:39 to 07:20 and again from ~12:00.
- Cause, from the disassembly: one read of the learn producer index (BAR0
  `+0x4860`) returned a non-index value. The count underflows, the NULL from
  `malloc` goes unchecked, and `while (sw != hw)` can never end (V once the
  value is in).
- The bad value was probably all-ones from a PCIe completion timeout (I).
  The root port 0001:00:00.0 has sticky AER `CmpltTO+ UnxCmplt+`, first
  error pointer 0x0e, which cannot be dated. It was deliberately left
  uncleared.
- With `pcieErrorsAsAllOnes` such a read returns 0xffffffff instead of the
  SError panic it would have caused before.
- Live evidence is in `analysis/research/ifcs-log-20261110/live/`.
- SDK restarted at 16:11 on the patched system: bring-up OK, 32 fpp.

**Fixes (nix, deployed 16:11 and 16:33):**
- The SDK /tmp is now its own 256M tmpfs, and the `cs8210-sdk-logtrim`
  service punches holes into any `*.log` over 16 MiB, keeping the first
  1 MiB and the last 4 MiB. Tested on tmpfs with a 10 MB/s non-append
  writer.
- SSD: journald wrote 6-13 MB/min, all mmap writeback (`wchar` 0).
  `SyncIntervalSec` 15 s -> 5 min alone still left 2.5 MB/min. A burst test
  (500 messages = +4.5 MB, ~9 KB each, against 2.2 MB/min quiet) showed that
  the cost scales with the number of writeback windows that contain a
  message, not with log volume.
- dhcpcd logged ~4 info lines/min, from two routers announcing the same ULA
  route. Its unit is now capped at `LogLevelMax=notice`.
- The remaining trickle is the hand-started `cs8210-powerlog` soak unit
  (every 30 s), which was left running.
- Measured over 10 min each:

  | | journald | sda total |
  |---|---|---|
  | before | ~13 MB/min | 16.5 MB/min |
  | 5 min sync | 2.5 MB/min | 3.3 MB/min |
  | + dhcpcd cap | **1.2 MB/min** (23 messages, 19 of them powerlog) | 2.0 MB/min |

  The rest of sda is jbd2 and the java process.

### 2026-11-10 (m) — FPGA simulation: empty-cage prediction, stretching, NACK, long reads, queue, IRQ, formal check

Offline only; no switch access (main may run a live empty-cage read in
parallel). Report [fpga/i2cfpga/SIM.md](fpga/i2cfpga/SIM.md) (new sections
"Hardware comparison", "Further behaviour", "Formal check"); plain language
in [fpga/i2cfpga/CHANNEL.md](fpga/i2cfpga/CHANNEL.md). `modules.py` and the
round trip were not changed this round. All results are simulation of the
recovered netlist (I for the model; see SIM.md).
- **Empty-cage prediction (for the live test):** read or 1-byte write at
  0x50 with nobody on the bus: wire `START 0xA0-NACK STOP`, busy clear
  ~1,910-1,930 clocks (30.5-30.9 us) after the start write, `cb+0x60` =
  `0x00000002`, `cb+0x54` = offset with bit 31 clear, `cb+0x50` reads back
  as written, the window untouched (keeps old data). Table with power-up /
  after-`0x0C` / after-probe values in SIM.md. The simulated version
  register reads `0x010b0009`, the live unit's value. At power-up `0x178`
  = `0xFFFFFFFF` (all modules held in reset) and the presence IRQ is masked;
  a `0x0C` bit-31 write restores the channel's profile defaults (`0x00081766`).
- **Clock stretching:** the engine waits; a stretch longer than ~63,000
  clocks (~1.01 ms) aborts with STOP and status bit 1, independent of the
  profile.
- **NACK of a written data byte is ignored** (status 0x1, remaining bytes
  still sent); NACK of the address or offset byte is detected.
- **Long reads:** 128 bytes land in window bytes 0-127 in bus order; each
  slot has 128 bytes (slot 0 = 0x000-0x07F, slot 1 = 0x080-0x0FF of both
  windows); a 255-byte read wraps and overwrites; 128 is the useful maximum.
- **Queue:** slot 1 started while slot 0 runs waits and runs afterwards
  (status 0x11); reversed order likewise.
- **Presence IRQ:** insertion sets `0x140`/`0x150` and raises one MSI
  request (`CFGINTERRUPTN` pulse, DI = 1) unless masked in `0x158`; `0x150`
  clears with ones **only while masked** (the vendor handler masks first).
- **Formal (`equivcheck.py` + yosys):** the shared region of the two
  templates (pad-driver cone, 132 elements) is proven equivalent by
  induction under a cut that assumes 124 differing pins equal; keeping 14 of
  those pins on their own drivers the check fails. Not a channel
  equivalence proof; a whole-design miter was not attempted (SIM.md says
  what it would need). The template's family map pairs the pads crosswise
  (logged; tools now seed by role).
- Not done: the vendor file's channel map (item 8).

### 2026-11-10 (l) — FPGA: full-design simulation, controller tables, families equivalent, sideband pins, vendor file

Offline only (no switch, serial or outlet). Reports
[fpga/i2cfpga/SIM.md](fpga/i2cfpga/SIM.md) (new),
[fpga/i2cfpga/LAYER6.md](fpga/i2cfpga/LAYER6.md),
[fpga/i2cfpga/CHANNEL.md](fpga/i2cfpga/CHANNEL.md). `ai_re/flake.nix` gained
`verilator` and `yosys` (simulation of the recovered netlist; yosys was
added for an equivalence check that in the end was not run).
- **Byte identity, re-verified with `cmp` in this session:**
  - vendor `i2c_fpga_v01.02.0E_20200604.bin` through layers 1-6 (fresh
    run, image0 + image1): `BYTE-IDENTICAL (layer 6)`, 8,388,668 bytes,
    SHA-256 `3b8c6c73...` on both files;
  - flash dump with the new layer-6 membership growth (`REUSE=1
    roundtrip6.sh`, layers 1-5 from the fresh run of entry (k)):
    `BYTE-IDENTICAL (layer 6)`, SHA-256 `fa559a02...`.
- **Full-design simulation** (`simnet.py`, `simtb.py`, `simrun.sh`):
  the recovered user logic (21k FFs, 31k LUTs, PCIe core included) runs in
  Verilator with PCIe TLPs in and an I2C device model on one cage. A
  driver-style read of 4 bytes runs end to end (`S A0 10 Sr A1 B5 B4 B7 B6
  P`, ACKs/NACK as expected, busy bit, status, read window in host order);
  writes, address NACK, the second command pair and a bus-clear command too.
  SCL measured on real transfers: 162 / 72 / 632 clocks = **386 kHz / 868 kHz
  / 98.9 kHz** for codes 0x18 / 0x09 / 0x66, confirming layer 7; bytes back to
  back. Model assumptions (TRN conventions, synchronous resets, ideal device)
  are I (SIM.md).
- **Status and command bits (simulation):** `0x60` bit 0 = slot 0 done,
  bit 1 = slot 0 NACK (the driver's -ENXIO), bits 4/5 = slot 1; cleared by
  the next start and write-1-to-clear. `0x54` bit 31 = busy. Command bit 28
  read, 15:8 address, 7:0 length (0 on a write does not stop), bit 29 on a
  write = bus clear (9 clocks + STOP), bits 23:16/31 no visible effect.
  After the `0x0C` bit-31 reset the channel wipes its BRAM for ~512 clocks
  and drops window writes meanwhile.
- **State tables** of the transaction controller (one-hot, 8 states +
  slot pointer), the byte sequencer (address / write / read / stop phases)
  and the bit sequencer: `fpga/i2cfpga/mtd5/i2c_channel_fsm.txt`, also in
  `i2c_channel_B.v`.
- **Families A and B are functionally equivalent by co-simulation:** a
  family-A channel (driver ch 1) and a family-B channel (driver ch 17), same
  15 directed + 10 random sequences, logs identical cycle for cycle in all
  25 runs. **Formal equivalence was not done.** Layer-6 membership growth
  (604 core nodes) raised the common template part from 776 to 813 elements
  but does not make the two templates cover the same element set.
- **Sideband per port** (`fpga/i2cfpga/mtd5/update.port-pins.txt`): all 32
  presence, IntL, LPMode and ResetL pins located through `0x140/0x160/
  0x170/0x178` bit k = port k+1; LPMode and IntL act only while the module
  is present; input N15 halts the PCIe core while low (reset-like). With the
  I2C channel map every port's six FPGA pins are known.
- Failures: asynchronous resets do not settle in Verilator (modelled at the
  clock edge, I); several testbench bugs on the way (logged in SIM.md); the
  vendor file's channel map/family split could not be decoded (its family
  split 7/25 and 4/28 is a layer-5 matching artefact, not the flash's 16/16);
  growth renumbers template element ids (docs use current ids + physical
  BELs); register-file slices and PCIe BRAM groups not re-rolled.

### 2026-11-10 (k) — FPGA layers 6 and 7: the 32 I2C channels re-rolled into one module, channel logic decoded

Offline only (switch, serial and outlet untouched). Reports
[fpga/i2cfpga/LAYER6.md](fpga/i2cfpga/LAYER6.md) (method, numbers, limits)
and [fpga/i2cfpga/CHANNEL.md](fpga/i2cfpga/CHANNEL.md) (plain-language
explanation); views `fpga/i2cfpga/mtd5/i2c_channel_{A,B}.v`.
- **Layer 6 (`modules.py`), lossless.** The 32 channel copies are factored
  into one module `i2c_channel` with two templates (family A 921 elements,
  family B 1,279; 776 correspond) + 32 instances (placement, port bindings,
  packing choices, routing, explicit deviations) + the non-channel
  remainder. LUT INITs, FF/control/carry lines and BRAM/IOB site features
  are regenerated from the template (48,647 of 133,986 cell lines; 19,556
  per-instance packing lines; 7,577 raw deviation lines; the rest is
  non-channel). Per instance 69-96% is pure template (median A 76%, B 86%).
- **Byte identity, fresh run in this session**: `fpga/i2cfpga/roundtrip6.sh`
  (layers 1-5 from the image, then template + instances + remainder ->
  netlist -> FASM -> frames -> image) gives `BYTE-IDENTICAL (layer 6)` for
  the 32 MiB flash dump (golden and update image both through layer 6;
  netlist, FASM and frames equal at every stage); `cmp` re-run by hand, SHA-256
  `fa559a02...` on both. The vendor 8 MiB file was not run through layer 6.
- **Layer 7 (`channelrtl.py`), a readable view.** Decoded from the logic:
  - register offsets (BDD of the write enables with the address bits as
    variables): `cb+0x00` = three 8-bit profile codes, `0x08` = 2-bit
    select, `0x0C` bit 0 master reset and bit 31 soft-reset pulse, `0x50`/
    `0x54` and a second pair `0x58`/`0x5C` with "go" flags on bit 31 of
    `0x54`/`0x5C` (V); simulation of the template writes 0x00660918 as
    profile 1/2/3 = 0x18/0x09/0x66 (power-up 0x66/0x17/0x08).
  - **driver channel of every instance** (from its decoded register page):
    all 32 distinct, 0..31; pad-pair ch21 = driver channel 17, ch25 = 0, ...
    (`mtd5/update.channel-map.txt`). This answers the open "ch -> port"
    question offline.
  - SCL divider: 16-bit down-counter reloaded with the selected code, a tick
    every code+1 clocks, held while SCL is stretched (V); bit-level FSM: a
    6-bit phase register, 6 ticks per bit, 11 for START (V); its four
    commands identified in simulation as START / STOP / bit-1 / bit-0 (I).
    Simulated SCL at 62.5 MHz: code 0x18 -> 386 kHz, 0x09 -> 868 kHz,
    0x66 -> 98.9 kHz (I; 6x(code+1)+12..14 clocks per bit, not 4x(code+1)).
  - The **two families** are the same engine with a different cut (V for
    the parts compared): family B decodes its own register page, family A
    receives finished register strobes from the core and has part of its
    divider/profile logic outside its block. Families: A = driver channels
    0-9, 21, 27-31; B = 10-20, 22-26.
- Not done / failed: a full transaction simulation through the shared PCIe
  core (the strobe timing of the core was not reproduced; the engine was
  driven at the bit-command level instead); the byte sequencer and
  transaction controller are not tabulated; family A's register map is
  partial; sideband / register-file / PCIe-BRAM re-rolling not started.

### 2026-11-10 (j) — FPGA layers 4 and 5: lossless netlist, 32 I2C channel engines

Offline only (switch and outlet untouched). Work in [fpga/](fpga/README.md);
reports [fpga/i2cfpga/LAYER4.md](fpga/i2cfpga/LAYER4.md) and
[fpga/i2cfpga/BLOCKS.md](fpga/i2cfpga/BLOCKS.md).
- **Layer 4 (netlist) done, lossless.** `fasmnet.py` turns the FASM into
  cells (sites with their FASM lines), nets (driver pin -> sinks, with the
  pips) and an `unassigned` bucket, using a routing graph built from
  prjxray-db (`xc7graph.py`, ppips handled: `always` followed, `default` =
  IMUX VCC tie, `hint` left to the site model). Flash update image: 458,693
  FASM lines, each owned exactly once (cells 133,986, nets 324,707,
  unassigned 0); 8,652 cells, 49,792 nets.
- **Byte identity re-verified in this session**: `fpga/i2cfpga/roundtrip4.sh` (image -> layers 1-3 -> netlist
  -> split into one file per block -> merge -> FASM -> frames -> image) gives
  `cmp`-identical images for the 32 MiB flash dump (golden + update) and the
  8 MiB vendor file (image0 + image1); netlist -> FASM is identical for all
  four images, 0 unassigned lines.
- Logical view `xc7bels.py`: 47,491 UNISIM-like primitives (25,237 LUTs,
  21,565 FFs, 270 CARRY4, 48 RAMB18, ...), no unconsumed SLICE feature;
  structural Verilog in `fpga/work/l4/update.v`.
- Bug found and fixed on the way: prjxray's `IOB_Y0` is the *upper* site of
  an IO tile. With the naive mapping the pad pairs were swapped, which looked
  like dead I2C pads; with the fasm2bels rule the IO ring is uniform.
- **Layer 5 (blocks), first pass.** Every cell/net/primitive gets a block
  path. Strongest results:
  - PCIe: PCIE_2_1 gen2 x1 with a 64-bit TRN interface; vendor/device
    `1d0f:8571`, rev 01, subsystem 0000:0000 are fabric ties (V); 2 x 8
    RAMB18 on the MIM ports = TX retry / RX buffers (V); MMCM + BUFGCTRL
    pclk mux + DCLK BUFG + /16 user clock = Xilinx `pipe_clock` topology
    (V topology, I name); all user logic on one 62.5 MHz clock.
  - IO: 64 open-drain IOBUF (T from FDPE INIT 1), 64 LUT-driven OBUF, 72
    IBUF (V); the 64 open-drain pads pair into exactly 32 mutual-best pairs
    (V).
  - Channels: lockstep structural matching from the 32 pad pairs matched
    28,720 logic nodes; every channel has one RAMB18 (SDP x36, 8 address
    bits from the channel; fits the 512 B + 512 B windows, I). Surprise:
    the channels come in **two structural families of 16** (about 390 vs
    540 FFs per channel); cause unknown.
  - Not achieved: tying register addresses (0x140..0x178, 0x1000+ch*0x100)
    to decode logic. Write-enable cones are 49-61 FFs deep with tens of
    cubes; no address constant could be read off. Channel numbers are
    therefore pad-pair order, not driver ports.
- fasm2bels cross-check: chipsalliance f4pga-xc-fasm2bels runs after two
  import stubs and one runtime patch; with assertions on it stops on the GTP
  refclk BUFG input having two sources (the same node our netlist flags),
  with `-O` it finishes, and its primitive counts match ours up to
  explained modelling differences (`LAYER4.md`).

### 2026-11-10 (i) — SError panic analysis, PCIe all-ones mode, enp0s3 up, FPGA round trip

- **Panic at 01:22** (boot uptime 3175 s), captured on serial: `SError Interrupt on
  CPU6 ... Comm: ifcs-learn` (SDK MAC-learning thread, userspace), ESR
  `0xbf000000`, `Kernel panic - not syncing: Asynchronous SError`. An ASIC
  BAR0 access went unanswered. Console showed no IPD-ERR and no AER/PCIe errors
  before it; the on-disk journal stops 77 s earlier (now synced every 15 s).
  CPLD fault log: nothing at crash time; the `XP12R0V_CPU_OK / VoltageIssueA`
  entry seen afterwards appears after every outlet power-off (12 V dropping),
  here at my 01:24 power cycle. Root cause of the ASIC going deaf **not
  established**. Only correlate: it was the one boot in which I had pulsed
  `asic_reset` three times; the pulse is now opt-in (`off --reset-asic`).
- **Mitigation**: DWC "AMBA error response" register (root-port config space
  `0x8D0`, Annapurna HAL `enable_axi_slave_err_resp`) was `1` on all three
  ports. Set to `0` (HAL-documented: return 0xFFFFFFFF without error) by
  `cs8210-pcie-errors.service` at boot. Verified: a BAR read with memory decode
  off, a guaranteed SError before, now returns `0xffffffff`.
- Steady-state power with the stack running ~700 W, flat; ASIC temperature
  creeping 59 -> 61 C at 50% fans. A soak logger (`/root/powerlog.sh` via
  `systemd-run --unit=cs8210-powerlog`, journal) samples every 30 s.
- **enp0s3 (second al_eth, 00:03.0)**: no PHY, vendor DT `port3 mode =
  "10g-serial"`. al_eth now runs a port without link management when there is
  no SerDes object (U-Boot's SerDes setup is used as is), carrier from MAC link
  status. Its far end is **ASIC devport 129** (`type AUX`, 10G, 1 lane, ISG
  32), which the vendor config defines with `admin_state FALSE`; `port enable
  129` (now in `cs8210-switch-up`) brings both ends to LINK_UP. Never leaves the
  chassis. enp0s3's MAC is random per boot (as in stock).
- **ethtool**: the driver's ethtool table was compiled out (legacy
  get_settings API). New minimal ops (drvinfo, link, link_ksettings): enp0s1
  reports 1000/Full via phylib (88E1510 is gigabit-only, matches the UniFi);
  enp0s3 reports from MAC mode.
- **FPGA**: full round trip flash -> packets -> frames -> FASM + 3 unknown bits
  -> back is BYTE-IDENTICAL for the flash image and the vendor file
  (`fpga/i2cfpga/roundtrip.sh`, report `fpga/i2cfpga/LAYER2.md`). Fabric proven
  to be prjxray xc7a100t (all 9448 frames match exactly once). Update image:
  ~25.1k LUTs, ~21.6k FFs, 48 RAMB18 (empty), PCIe gen2 x1 hard block, one
  MMCM, 218 IOs (64 bidirectional, likely 32x SDA/SCL). Package FGG484 or
  FGG676 (chip marking not read).

### 2026-11-10 — WorldEdit installation

- Installed WorldEdit 7.4.6 Bukkit release for Paper 26.3, from EngineHub's
  Modrinth project. Download SHA-512 matched upstream metadata; SHA-256
  pinned as flake packages.worldedit. Provenance in papermc/worldedit.json.
- Built on switch, rooted at /root/papermc/worldedit-package; plugin symlink
  /root/papermc/plugins/worldedit.jar. Stopped server gracefully to install,
  then restarted. No world edits performed.
- Switch rebooted externally during verification. After it returned, the DHCP
  wait and automatic startup succeeded. RCON worldedit version confirmed
  7.4.6 with WORLD_EDITING capability; plugins listed WorldEdit enabled.
  cuttlefishCuller rejoined successfully.

### 2026-11-10 — Minecraft console and open admission

- User requested an admin console, no whitelist, and op for cuttlefishCuller.
- Added just console / just command via mcrcon, exposed in the flake and
  devshell and installed in root's profile. Generated a random RCON password
  only on the switch; server.properties mode 0600. RCON TCP 25575 remains
  blocked from the LAN; use SSH then just console.
- Whitelist and enforcement disabled; online-mode remains true. No router
  forwarding added. RCON confirmed op and whitelist status.
- Persisted level-4 operator cuttlefishCuller, UUID
  7a61f29a-5d83-401f-bc3d-b616152c836b, in live ops.json.

### 2026-11-10 — PaperMC human controls and DHCP startup fix

- Added papermc/justfile with start/stop/restart, status, logs/recent, memory,
  join, build/deploy, and boot-status. Exposed just as a flake package and
  added a devshell with just/Python. Installed just in root's Nix profile.
- Deployed flake, justfile, and docs to /root/papermc; menu and read-only
  recipes validated on the switch, start recipe exercised.
- Switch rebooted during this work (not initiated here). The persistent boot
  dependency worked, but Paper exited after failing to bind before DHCP
  assigned 10.60.2.190. Added a bounded address wait before server startup;
  started the service again. No reboot test of the new wait performed.

### 2026-11-10 — PaperMC side quest

- User requested a standalone Nix flake deployed to `/root/papermc` on
  `10.60.2.190` with a 4 GiB memory budget.
- Deployed pinned Paper **26.3-223** / Java 25 on aarch64; sources and
  operations: [papermc/README.md](papermc/README.md).
- Systemd service caps total memory at 4 GiB (3.5 GiB high watermark,
  swap disabled); heap 2560 MiB, four JVM-visible processors, low CPU weight.
- Survival, six slots, view/simulation 6/4, online authentication.
  Management address TCP 25565 only; labeled INPUT rule managed by service.
- Persistent unit and multi-user target dependency installed under
  `/etc/systemd/system.attached`; confirmed target Wants includes papermc.
  No NixOS rebuild, hardware changes, or reboot.
- First startup reached Done in 33.467 seconds; external Minecraft status
  probe returned Paper 26.3 (protocol 777), 0/6 players, correct MOTD.
  Memory ~1.58 GiB after world creation, no automatic restarts.
- Default user's SSH key was unavailable; used the documented local
  `sudo ssh` root key. Initial devshell path-source attempts stalled;
  normal `nix develop` worked once sandbox access was restored.
- Full in-game login/load testing remains unperformed.

### 2026-11-10 (h) — FPGA reverse engineering started (byte-identical first)

User goal: byte-identical rebuild of the FPGA images from a readable form,
then make it progressively more meaningful. Work lives in [fpga/](fpga/README.md).
- I2C FPGA flash (`mtd5`) is a multiboot image: header (watchdog + IPROG to
  `0x400000`), compressed golden image, compressed update image at
  `0x400000`, erased from `0x801000`. It differs from the SSD's shipped
  `i2c_fpga_v01.02.0E_20200604.bin` (older); flash is the target.
- **Layer 1 done**: `fpga/i2cfpga/bitasm.py` packet listing <-> image,
  BYTE-IDENTICAL for both the 32 MiB flash image and the 8 MiB shipped file.
- prjxray-db has no XC7A75T; expected to be the XC7A100T die (like 35T ->
  50T). Layer 2 (frames, re-packing encoder, FASM + unknown bits) handed to
  an agent with the instruction to prove the fabric match from frame
  addresses rather than assume it.

### 2026-11-10 (g) — power budget, data-plane power CLI, QSFP tool

- **Power** (PSU1 PMBus, only PSU fitted; 117 V in): **~696 W at the wall**
  with the switch stack running idle, no optics. Regulator telemetry
  (TPS53688 READ_POUT, PAGE select + read): ASIC `vddcore` 127 W (159 A),
  `avdd` 88 W, `avddh` 25 W (ASIC ~240 W); Credo/MACsec rails ~165 W; PSU
  loss ~42 W; remaining ~220 W fans, CPU card, FPGAs, misc.
  Stopping the SDK alone changes nothing.
- System CPLD `0x03` bit 5 = vendor `asic_reset` (active low, from
  `sys_cpld.ko` attribute index 24 and the `cpld_set_data` jump table). **It
  is a pulse**: the bit returns to 1 within ~5 s, so the ASIC cannot be held
  in reset. After the pulse its core rail drops only 128 -> 118 W; it is
  leakage-dominated. No ASIC rail-enable exists in the CPLD map.
- Credo chips: MDIO FPGA `0x80000 + 0x20*(N-1)` bit 0 (vendor `phyN_reset`,
  32-bit RMW). In reset: Credo rails 165 -> 123 W, **wall 696 -> 631 W**.
- `cs8210-switch-power off|on [--start]|status` (nix vendor-sdk): stop SDK,
  Credo reset, unbind + PCI-remove the ASIC, reset pulse; `on` releases,
  rescans PCIe and rebinds ipd (no reboot needed, verified twice, SDK and
  Credo come back, 32 netdevs). Saves ~65 W.
  Bigger savings would need PMBus OPERATION-off on the ASIC/MACsec
  regulators: not attempted (rail sequencing / back-powering risk, and the
  CPLD's voltage-fault action `0x24` is undocumented).
- **QSFP cages** ([protocol review](reviews/2026-11-10-i2c-fpga-protocol.md),
  agent, offline): register-style I2C engine, per channel control block
  `0x1000 + ch*0x100`, write window `0x4000 + ch*0x200`, read window
  `0x8000 + ch*0x200`, command `cb+0x50`, start/busy `cb+0x54`, status
  `cb+0x60`, all 32-bit, completion polled. Tool `cs8210-qsfp` in the nix
  tools. First read-only run on hardware OK: no modules, all cages
  `reset=1`, channel profile `0x00081766` (vendor probe would write
  `0x00660918`, `cb+0x0c=0x80000000`, `0x178=0`). Apply with `--init` /
  `--reset P 0` when the first module is in.
- FPGA images: the I2C FPGA bitstream IDCODE is `0x3632093` (believed
  Xilinx Artix-7 XC7A75T). The other programmables are Lattice `.vme`
  images (not identified yet).

### 2026-11-10 (f) — module support survey, QSFP cage sideband state

- What the cages take (vendor `dynamic_config.sh` / Credo lane tables):
  1x400, 1x200, 2x200, 2x100, 4x100, 8x50 (QSFP-DD); **1x100 (QSFP28)**,
  **1x40 (QSFP+)**, and 4x25 / 4x10 breakouts. No single-lane 1x10 / 1x25
  entry, so SFP+ through a QSA adapter would have to run as sub-port 1 of a
  4x10 port (inferred, untested). Nothing below 10G per lane. No module
  vendor/part-number checks found in the SDK.
- User ordered QSA adapters and DACs (2026-11-10).
- I2C FPGA sideband, read-only on mainline (32-bit BAR0 reads, as the vendor
  driver does): `present 0`, `present_changed 0`, `interrupt 0`,
  `interrupt_mask 0xffffffff`, `lpmode 0`, **`reset 0xffffffff` (every cage
  held in reset)**. The vendor IRQ handler writes 0 to `0x178` on a presence
  change; nothing on mainline does that yet, so plugged modules would stay in
  reset. Polarity of present/reset inferred from the vendor tools, to be
  confirmed with the first module.
- Agent working offline on the I2C master transaction protocol (module
  EEPROM reads, ideally a mainline i2c_adapter).

### 2026-11-10 (e) — CPU RX errors explained and fixed; ASIC temperature; fans back to ~50%

- **`Unsupported VF 3` / `Invalid LDH` was not a driver bug.** A temporary
  debug build hex-dumped the rejected frames: all on CPU RX ring 3, all plain
  Ethernet with no switch header, all IPv6 (MLD reports, NS for DAD, RS) from
  `5e:94:49:e4:a2:48`, the MAC of the SDK-internal netdev `inno_0_1000`. The
  host's own frames on that interface come straight back. The exponential
  spacing was router-solicitation backoff. Stock ONIE never sends on it.
  Fix: udev sets `disable_ipv6=1` on `inno_*`, lldpd limited to
  `enp0s1,fpp*`. Result: 0 errors over 2.5 min (was ~20 in the same window);
  lldpd now covers enp0s1 + fpp1..32.
- Fixed my own bugs in the service: `cs8210-switch-down` never ran (a
  makeWrapper `--argv0` does not change `$0` of a bash script; now `--down`),
  and each chroot entry had a private `/tmp`, so `phy_shell exit` could not
  reach `phy_server`'s fifo. `/tmp` and `/var/run` of the chroot are now
  shared under `/run/cs8210-sdk`. Start, stop (9.5 s) and restart are clean.
- **ASIC die temperature on mainline**: MCP3425 ADC at `0x68` behind
  `0x70:2 -> 0x74:0 -> 0x72:7` (Linux bus 21 here), reading the ASIC thermal
  diode. Conversion recovered from the vendor `check_switch_temp`
  (disassembly, float constants): `T = -124.28 V^2 - 422.03 V + 384.62`,
  V = raw / 1000 (power-on config `0x10`: continuous, 12 bit, gain 1).
  Reads 52-59 C idle with the SDK running (59 C steady at ~50% fans).
- Board curve now starts at 40 C (MACsec card LM75s idle at ~38 C; vendor
  board warning 53 C), so the fans sit at the 50% floor at idle. In `cs8210-show temperature` as
  `switch-core (ASIC)`.
- **Fans**: system CPLD `0x05` bits 7/5 (active-low fan_max_speed) had been
  cleared, forcing 100%. Setting them back (what the vendor's install.sh does)
  sticks. `cs8210-fancontrol` now reads the ASIC temperature every cycle,
  applies a second curve (70 C -> min, 95 C -> full; vendor PID setpoint is
  97 C, limit 105 C), releases the CPLD force only while that sensor reads
  plausibly, re-writes PWM if the CPLD overwrote it, and on any error or stop
  sets PWM 255, re-clears the CPLD bits and re-arms the fan watchdog.
  Verified cold boot with the ASIC running: 54-58% (~11.5k rpm, was ~19k);
  stopping the daemon -> CPLD `0x05=0x40`, 100%; restarting -> back to 54%.

### 2026-11-10 (d) — switch ASIC online on mainline: fpp1..fpp32 netdevs

- **Root cause of `Node 0 couldn't be created`** (debug agent, on hardware,
  [2026-11-10-ipd-legacy-abi-debug.md](reviews/2026-11-10-ipd-legacy-abi-debug.md)):
  `libifcs.so` (`_ii_pcie_attach`) sscanf's the version string the driver
  returns from QUERY_NODE and compares major/minor/patch with its own
  (0.12.41); the legacy patch returned upstream's `0.16.2.0.0.0`. The mismatch
  is only logged above the SDK's default log level. Fix: module parameter
  `legacy_version` (default `0.12.41.1.0.1`) reported in legacy mode.
- **Result, from a cold boot** (`systemctl start cs8210-switch`, 49 s):
  `Node 0 created and enabled.`, `Knet config done`, **`fpp1`..`fpp32`**
  (MTU 9462, MACs `0e:48:c6:7a:55:cf`..`ee`, UP / NO-CARRIER with no optics)
  plus `inno_0_0129` and `inno_0_1000`; 8 MSI-X vectors (`inno_irq-0-napi` ..
  `7-rx-mcu0`, only `pic` counting); Credo: 16/16 slices firmware `01.92.02`,
  `credo config done!` in MACsec bypass. No oops or SError.
- New in the nix tree: `cs8210-switch-up [mode]` / `cs8210-switch-down` and
  `cs8210-switch.service` (not enabled at boot on purpose); DHCP restricted to
  `enp0s1` so the host does not auto-configure the front-panel netdevs.
- **Open problems**
  - `IPD-ERR: Unpack-Unsupported VF 3` (or 7) / `Invalid LDH` on the CPU RX
    ring, about one per frame the host sends on `inno_0_1000` (16 in the first
    minute, RX counter stays 0). The 0.16.2 v0 header parser rejects something
    the old module accepted, or the legacy SET_NETDEV translation is off.
    **CPU-path receive is therefore not proven.** First thing to fix before
    any traffic test.
  - **Fans are at 100% once the ASIC has been initialised**: system CPLD reg
    `0x05` went `0xe0 -> 0x40` (bits 7/5 clear = forced max) and stays so
    across a warm reboot; the fan daemon's PWM writes read back as 255. Left
    alone deliberately: there is **no ASIC die temperature on mainline yet**.
    The vendor reads it from an MCP3425 ADC (`0x68` behind `0x72` ch 7) through
    a `check_switch_temp` helper, and Q16 from the ASC10 monitors; our kernel
    has IIO disabled and no ASC10 driver. Board LM75s read 30-35 C.
  - The netdevs are the SDK's CPU-path interfaces ("knet": each port in a
    private VLAN with the CPU port). Nothing is offloaded from Linux; no
    switchdev. No link has been tested (no optics fitted).
  - Aux SPI flash, second NIC, kernel-side MDIO/I2C FPGA drivers: unchanged.

### 2026-11-10 (c) — old SDK against the new ipd: legacy ABI patch, first live runs

- Compat verdict (agent,
  [2026-11-10-ipd-source-compat.md](reviews/2026-11-10-ipd-source-compat.md)):
  ipd 0.16.2 cannot serve the 2020 userspace as-is. Same ioctl numbers and
  header version, same mmap convention and Teralynx-7 header path, but 11
  ioctl argument structs grew, device init moved from `open()` to new ioctls
  the old library never calls, the node is `/dev/ipd0`, and INFO_NODES no
  longer sets `num_nodes`. The agent's patch adds `legacy_abi=1` (old struct
  layouts with size asserts, init at open, node named `ipd`). In the nix tree
  as `kernel/patches/ipd-legacy-abi.patch`, with `options ipd legacy_abi=1`.
- Marvell's newer SDK userspace (`sonic-marvell-teralynx-binaries`, SAI up to
  7.0.1) is **amd64 only**, so it cannot replace the arm64 SDK on this box.
- Live result so far, patched module loaded with `legacy_abi=1`, vendor
  userland in the chroot (needed `netaddr` copied into python2.7
  site-packages; ONIE installs it from a tarball at boot):
  the driver opens, initialises the chip (swap status 0, SYNC_MODE 1, **MSI-X
  8 vectors**, DMA init done), answers INFO_NODES and one QUERY_NODE, then the
  SDK prints `ERR: Node 0 couldn't be created, rc: IFCS_DRIVER_ERROR` and
  closes the device. No netdevs yet. No kernel errors, no crash.
  A debugging agent with ssh access (strict no-MMIO / no-reboot rules) is
  working on what the library rejects after QUERY_NODE.
- Plumbing mistakes of mine in this stretch, for the record: a bad scripted
  edit truncated `libMdio.c` (rewritten); `pkill -f` matched and killed my own
  ssh command twice; `modprobe` after `switch-to-configuration switch` loads
  modules from the *booted* generation, so a new module has to be insmod'ed
  by path or the box rebooted.

### 2026-11-10 (b) — ipd driver source found and running; Credo PHYs up on mainline

- **What the FPGAs do** (agent review
  [2026-11-10-fpga-roles-and-datapath.md](reviews/2026-11-10-fpga-roles-and-datapath.md),
  offline analysis of the stock files): neither carries traffic.
  `1d0f:8571` (Xilinx, own card) is the QSFP-DD management plane: 32 I2C
  masters (one per cage) plus present / reset / lpmode / interrupt bits.
  `1d0f:ec30` (Lattice, on the MACsec card) is the Credo PHY management plane:
  16 MDIO masters plus per-chip reset/gpio/interrupt bits.
  **Only ports 1-16 go through Credo CMS50216 MACsec PHYs; ports 17-32 connect
  the ASIC straight to the cages.** Port map, lane swaps and the stock bring-up
  sequence are in the review.
- **The ipd kernel driver is open source** (user found the lead): Marvell
  publishes it under GPLv2 at `github.com/Marvell-switching/mrvl-teralynx`
  (`drivers/ipd`, version 0.16.2, commit `53d8dcf`, 2025-10-15), the driver
  SONiC builds. It lists `1d98:1b58` rev `0x10`. Built for 7.3.3/arm64 as a nix
  derivation with one shim (`strncpy` no longer exists in the kernel). On the
  switch it autoloads by PCI alias and logs `probe found PCI device 1d98:1b58
  revision - 10 ... initialized. Version - 0.16.2.0.0.0`; node is `/dev/ipd0`
  (dynamic major), with a udev symlink `/dev/ipd` for the old SDK.
  **Not yet known: whether the 2020 SDK userspace (built against ipd
  0.12.41) is ABI-compatible with this 0.16.2 driver.** The ioctl numbers do
  not encode struct sizes, so a mismatch would be silent; an agent is diffing
  the old module's DWARF layouts against the new source before the old
  `innovium.user` is pointed at it.
- **Credo MACsec PHYs brought up on mainline.** The vendor tools (`phy_server`,
  libcredo) run unmodified in a chroot of the ONIE root filesystem
  (`/var/lib/cs8210-sdk/root` on the switch, glibc 2.25). The only replaced
  piece is `c-phy/libMdio.so`: ours (`nixos/module/cs8210/vendor-sdk/libmdio`)
  drives the MDIO FPGA from userspace via the PCI resource file instead of
  the vendor's 4.9-only `/dev/mdio-N:XX` + sysfs stack; the vendor command
  files' `echo HZ > /sys/bus/mdio_bus/devices/mdio-N:XX/mdc_freq` lines land
  in plain files the launcher provides. Run:
  `c-phy/auto_load_user.sh -m 1-32:optics_1x400G:fec-def -e macsecbypass -l`.
  Result (`evidence/mainline-sdk-2026-11-10/credo-bringup.log`): 16/16 slices
  `[Firmware load] Version: 01.92.02` (the version the factory log shows), no
  errors or timeouts, `credo config done!`. Mode is **MACsec bypass**
  (retimer, no encryption) on purpose: the vendor default installs
  GCM-AES-128 SAs with a fixed test key printed in the script.
  PHY firmware lives in RAM, so this has to be re-run after every boot.
- New in the nix tree: `hardware.cs8210.vendorSdk` (ipd module, libMdio,
  `cs8210-sdk-chroot`). The vendor userland itself is deliberately not in the
  repo.

### 2026-11-10 — stock SDK reference run on ONIE (ASIC + MACsec PHYs online under the vendor stack)

User direction: find out what the FPGAs do, use the diag OS / ONIE / stock files
to chart a course, then get the switch chip and MACsec online on mainline with
the front-panel ports as (eventually offloadable) Linux interfaces.

Booted stock ONIE from the U-Boot prompt (`run onie_bootcmd`, RAM only; NixOS on
the SSD untouched) and ran the vendor bring-up by hand:
`./integrator_mode -m 32x400 -a 0E:48:C6:7A:55:CE -k -d` in
`/root/sdk/R3181-J0001-01_V0.1.7_Briggs_SDK`. Evidence in
`evidence/stock-sdk-run-2026-11-10/` (integrator.log 2773 lines, dmesg, state);
the installed SDK 0.1.7, drivers and diag configs were copied off the live
ONIE into `analysis/onie-live-2026-11-10/` (local-only).

What a working stack looks like on this unit:

- `ipd.ko` ("Innovium Switch PCIe Driver ... Version 0.12.41.1.0.1", 454 kB
  loaded) probes `1d98:1b58 rev 10`, creates `/dev/ipd` (char major 101).
  Userspace is `innovium.user` (one process, ~750 MB VSZ) started by
  `auto_load_user.sh`, config assembled from `cel_configs/*.yaml`, SKU file
  `configs/sku/innovium.77701_A` (Teralynx 7, IVM77700 family).
- **32 netdevs `fpp1`..`fpp32`** (renamed from `inno_0_0001`..), MTU 9462, MACs
  base+1.., plus `inno_0_0129` and `inno_0_1000`. No optics fitted, so carrier 0.
- ASIC interrupts: **MSI-X**, 7 vectors used of 32 (`inno_irq-0-napi`, `1-async`,
  `2-tx`, `3-rx`, `4-err`, `5-lrn`, `6-pic`); only `pic` fires when idle.
  PCI command: Mem+ BusMaster+ DisINTx+.
- `innovium.user` mmaps `/dev/ipd` with the **physical address as the mmap
  offset**: BAR0 (`0xc0200000`, 128 K), a few CMA regions (`0xaf...`) and
  several hundred single 4 K pages above 4 GiB (DMA buffers).
- Credo side: `phy_server` (from `c-phy/`) talks to char devices
  `/dev/mdio-N:addr` (major 240, one per MDIO master, created by the
  `mdio-fpga`/`mdio-lattice` modules), loads PHY firmware and configures the
  MACsec engines: log shows `DEV_EIP164_n` / `DEV_EIP163_n` (Rambus/Inside
  Secure MACsec IP: 1024 SAs, 512 SCs, 12 channels per device), ending in
  `credo config done!`.
- Mode string actually applied for `32x400`:
  `1-16:copper_1x400G;17-32:optics_1x400G` (the script forces host link
  training on ports 1-16), `boardID:0x01`.
- Temperatures after bring-up: hottest sensor ~50 C (ASIC powered and
  configured, no traffic), others 25-41 C.

Two offline agents are working from the stock files in parallel: FPGA roles /
port map / bring-up sequence, and a port specification for a mainline
replacement of `ipd.ko`.

### 2026-11-09 (latest+1) — FPGAs and all 16 MACsec PHY ports identified on mainline

Register maps came from a second offline agent reversing the vendor modules
([review](reviews/2026-11-09-fpga-asic-probe-maps.md)); every access below is
one the vendor drivers perform. Now `cs8210-pci-probe` / `cs8210-show macsec`
in the nix tools.

- **Both FPGAs leave U-Boot with PCI memory decode off** (`COMMAND=0000`). A BAR
  read in that state would have been a third SError panic. `echo 1 > enable`
  first; the stock kernel logs the same `enabling device (0000 -> 0002)`.
- mdio-fpga `1d0f:ec30` BAR0+0x0 = **`0x00001008`** (same as the stock dmesg).
  Its eight "phy misc" blocks read: reset released, gpio/irq/mask 0.
- i2c_accel_fpga `1d0f:8571` BAR0+0x0 = `0x010b0009`: version 0x0b00 (the stock
  driver calls itself v0.11), board version 9.
- **Credo CMS50216 on all 16 MDIO masters** (`mdio-1:02` ... `mdio-16:11`):
  clause-45 1.00cc = `0x2161`, 1.00cb = `0x00a2` (vendor `phy_id 0x216100a0`
  with a mask). Read through the FPGA's MDIO master registers with the "new"
  completion handshake; sys CPLD reg `0x01` = `0x10` selects it. This needs
  16-bit writes to FPGA registers (bus reset, address, start, done-ack); no
  PHY register is written. Sys CPLD `0xa4 = 0xb0` (MACsec present),
  `0x06 = 0xff`, `0xa5 = 0xff` (the diag YAML expected `0x3f` under a mask).
- Innovium `1d98:1b58` rev `0x10`: identification is PCI config only. The
  vendor `ipd.ko` probe never reads BAR0, and its first BAR0 traffic at open
  time is a series of writes, so nothing further was read from the ASIC.
- al_eth: `enp0s1 selects TX queue N, but real number of TX queues is 4` was
  logged a few hundred times per boot (queue id = CPU number, 16 CPUs, 4
  queues). Fixed with a modulo in `al_eth_select_queue`; zero after reboot.

### 2026-11-09 (latest) — TPM solved, lldpd fixed, ssh keys baked in

- **TPM works on mainline, from boot, declaratively.** Root cause found by a
  fresh-context agent disassembling the vendor kernel
  ([review](reviews/2026-11-09-tpm-aux-spi.md)): the TPM and the FPGA flash both
  sit on the aux controller's SS0 behind a board mux steered by SoC GPIOs
  **PBS52 / PBS54** (PL061 `gpio6` at `0xfd898000`, bits 4 and 6). The vendor
  `dw_spi_set_cs()` strcmp()s the SPI modalias, sets the two GPIOs and forces
  chip select 0; U-Boot leaves the mux on the flash at OS handoff. Confirmed live:
  `gpio6` data read `0xbb` (52=1, 54=0); after writing 52=0, 54=1 a re-bind gave
  `2.0 TPM (vendor-id 0x15D1, device-id 0x1B, rev-id 22)`.
  Fix is DT-only: `gpio6` PL061 node, `gpio-mux` on bits 4/6, `spi-mux@0` with
  `tpm@2`; modules `mux-gpio` + `spi-mux` loaded at boot. `tpm2_getcap` reports
  IFX SLB9..., `tpm2_pcrread` works.
  My earlier "no GPIO changes during transfers" observation was wrong in
  effect: I sampled within one device's traffic, where the mux does not move.
- **Aux flash (i2cfpga, n25q256a) still does not answer** with the mux at
  PBS52=1/PBS54=0: JEDEC reads all ones through spi-nor and by hand, in every
  mode/speed tried. PBS52=0 reaches the TPM regardless of PBS54. All eight GPIO
  banks read the same on mainline as under the stock kernel, so something else
  gates the flash. Node kept in the DTS with `status = "disabled"`.
- **lldpd**: the aarch64 build failure is lldpd's bundled, stale
  `include/linux/types.h` shadowing the system one; it lacks `__u128`, which
  current arm64 `asm/sigcontext.h` uses. Fixed with a 5-line patch in
  `nixos/machine/cs8210` (overlay). Running; sees the upstream switch on port 4.
- **ssh**: root's authorized keys are now the five keys from
  `https://github.com/nepeat.keys` (fetched 2026-11-09; the first is the
  existing erin-laptop key) plus `root@sea420-desktop`. The temporary bring-up
  key and my probe scripts were removed from the switch; the private half was
  deleted from the scratchpad. Note: `erin@sea420-desktop` has no ssh keypair,
  so from the desktop it is `sudo ssh root@10.60.2.190`.
- Second offline agent started on register maps for the MDIO FPGA / Credo
  PHYs, the I2C FPGA and the Innovium ASIC, so the first reads are ones the
  vendor drivers provably do (unacknowledged MMIO reads panic mainline here).

### 2026-11-09 (late) — TPM investigation, tools, state at end of session

Deployed over ssh (`nix copy` + `switch-to-configuration boot`), which also
exercised the bootmenu builder on the box: two generations listed, default boots.

Verified on the final system: `systemctl is-system-running` = running, no SATA
exceptions since `ahci.mobile_lpm_policy=1`, fan daemon active, 14 temperature
sensors with board locations (switch board, MACsec card, both fan boards,
i2cfpga card), CPLD versions read over I2C match U-Boot's banner (system 0x15,
switch LED 2, MACsec LED 1, fan 3/3), `fw_printenv` works against `/dev/mtd1`.

Corrections to earlier notes:

- The EEPROM at `0x57` on the root I2C bus is **blank** on this unit. The ONIE
  TlvInfo that U-Boot/ONIE show lives on the **switch board EEPROM**, `0x56`
  behind `0x70:2 -> 0x74:0 -> 0x71:7`. The i2cfpga card has an IPMI FRU EEPROM
  at `0x56` behind `0x71:0`. Neither is in the SPI flash `board_eeprom`
  partition, which starts with zeros.
- Each fan CPLD has 7 fan slots, not 6.

**TPM / aux SPI: still not working on mainline. What was established:**

- Stock kernel sees the TPM (SLB9670, `0x15D1/0x1B`) and the n25q256a on every
  boot path, including when booted with `booti` from the prompt (experiment D,
  stock kernel + stock DTB over TFTP). U-Boot's `al_tpm_tis20_end: write failed /
  TPM extend failed` lines appear on every `booti` and are a red herring.
- Not the cause: interrupting autoboot (A), `scsi init` (B), probe timing
  (re-binding `tpm_tis_spi` 55 s after boot still reads `0xFFFF`), pin mux
  (`pbs_mux_sel_4` at `0xfd8a8220` reads `0x30000333` under U-Boot, stock and
  mainline: pins 32/33/34/39 on function 3, as the public HAL's
  `al_muio_mux_iface_pins_spim_aux_v3` says).
- DTS now uses `amazon,alpine-dw-apb-ssi` for both controllers (what U-Boot's
  embedded DT uses; enables mainline's CS-override path). Boot flash still
  works, aux bus still reads ones.
- Under the stock kernel, sampled with `devmem` during real transfers: DW
  `SER` reads **1** for both TPM and flash traffic (the DT says flash is CS 1),
  `CS_OVERRIDE` (`+0xf4`) stays `0xF`, `CTRLR0=7`, no GPIO bank or SGPO word
  changes. (`gpio5` bit 4 toggles by itself even when idle.)
- A hand-driven transfer through `/dev/mem` (fill FIFO, set `SER`) returns the
  right JEDEC ID on the boot flash controller (`c2 25 39`) but all ones on the
  aux controller, **even under the stock kernel while its own driver works**.
  So the stock driver does something per transfer that is not visible in the
  registers sampled. The vendor kernel has `dw_spi_gpio_cs_control` /
  `cs-gpios` strings, but the vendor DT has no `cs-gpios`.
- Next idea: disassemble `dw_spi_set_cs` / transfer path in
  `analysis/kernel-review/vendor-Image`, or trace SPI pins with a logic
  analyser. `bootm` cannot be used to boot mainline for comparison: the 71 MB
  Image exceeds this U-Boot's `CONFIG_SYS_BOOTM_LEN`.

Mistakes worth not repeating:

- **Reading unimplemented PBS registers through `/dev/mem` panics mainline**
  (`Asynchronous SError Interrupt`). U-Boot and the stock kernel return
  `0xbad0add1` for the same reads. I did this twice (`0xfd8a8000` block, then
  the `0xfd8a8130` row, whose second word is unimplemented). Both needed a
  power cycle; no persistent damage seen.
- U-Boot `dhcp` overwrites `serverip` with the DHCP server's; set it afterwards.
- ONIE over non-interactive ssh has no `/usr/sbin` or `/sbin` in PATH
  (`fw_setenv`, `reboot`, `devmem` need full paths); `fw_setenv` needs `-f`.
- ONIE's dropbear accepts root without a key, so a script aimed at NixOS will
  happily run on ONIE if that is what is booted. Check `uname -r` first.

Left on the bench: switch powered on, running NixOS, fans at 50%. TFTP test
files in `/srv/tftpboot/cs8210/` on sea420-desktop (mainline and stock ONIE
kernel/initrd/DTB) can be deleted.

### 2026-11-09 (night) — mainline 7.3.3 boots, NixOS installed on the SSD

**Result: NixOS 26.11 with mainline Linux 7.3.3 boots unattended from the SSD
to a root shell on the serial console, with management Ethernet up.**

Sequence, including the failures:

1. RAM boot via TFTP from the U-Boot prompt (nothing persistent). First `booti`
   attempt failed: `Wrong Ramdisk Image Format`. This U-Boot does not accept a raw
   initrd as `addr:size`; it needs a mkimage legacy ramdisk. That also rules out
   `sysboot`/extlinux with an initrd, so the DMC12 bootmenu builder was ported
   after all (`module/cs8210/uboot-menu`): raw `Image` + `booti`, wrapped initrd.
   Load addresses: Image `0x08000000` (booted in place, header flags allow it),
   FDT `0x10000000`, initrd `0x20000000`, script `0x07000000`, menu `0x07100000`.
2. Second attempt booted to NixOS stage 1: console, 16 CPUs, memory map with all
   three vendor no-map regions, timer 75 MHz, internal PCI, **all three external
   PCIe ports linked** (Innovium `1d98:1b58` Gen3 x4, FPGA `1d0f:8571` Gen2 x1,
   MDIO/MACsec FPGA `1d0f:ec30` Gen1 x1), AHCI + SSD, boot SPI flash partitions,
   I2C mux tree.
3. Built a raw disk image (`system.build.sdImage`: 16 MiB FAT stub + ext4 root,
   grown on first boot), power-cycled to ONIE and wrote it to `/dev/sda` over ssh
   (`pigz | ssh | gunzip | dd`), verified by SHA-256 of the exact image length.
   **The factory DIAGOS on the SSD is gone**; it survives only in
   `images/ssd-2026-10-17.img.zst`.
4. First disk boot reached multi-user, then the kernel oopsed in
   `al_eth_lm_mode_change` (NULL SerDes object) when networkd opened the second,
   PHY-less NIC (`00:03.0`, 10GBASE-SR towards the switch board). That hung
   logins. Fixed in the vendored driver: refuse to open a port that needs link
   management when there is no SerDes support (V3 AVG SerDes is not ported).
5. al_eth on `00:01.0` also needed: PCI rev 4 -> effective rev 2 (as the vendor
   HAL does), clause-22 MDIO bus ops (the driver only registered C45 ones), and
   SGMII as the phylib interface. Result: `Marvell 88E1510`, 1 Gbps/Full,
   MSI-X per queue, DHCP 10.60.2.190, ssh works.
6. `fw_setenv -f nos_bootcmd 'scsi init; ext4load scsi 0:2 0x07000000
   /boot/boot.scr; source 0x07000000'` from ONIE. **This is the only firmware
   change made.** `uboot-env` md5 before `e8790bbe...`, after `fffb5a04...`.
   Stock `bootcmd` still ends in `run onie_bootcmd`, so ONIE is the fallback.
   To undo: `fw_setenv -f nos_bootcmd echo`.
7. Under write load the SSD threw `ata16: SError { PHYRdyChg CommWake }` and
   NCQ timeouts about once a minute (three in the first three minutes, with I/O
   errors). Cause: SATA link power management (`min_power_with_partial` from the
   kernel's mobile LPM default). With `max_performance` set live: 6 GB of
   fsync'd writes, zero new errors. Made permanent with
   `ahci.mobile_lpm_policy=1`.
8. Fans: applied the vendor quiet recipe over i2c-dev (fan watchdog reg `0x0c`
   `0x19 -> 0x18`, PWM regs `0x22..0x82` `0xff -> 0x7f` on both fan CPLDs). Tach
   raw 189/208 -> 112/123. Register map confirmed on hardware: CPLD version at
   `0x00` = 3, **7 fan slots per board** (the diag YAML comments out the 7th),
   sys CPLD `0x05 = 0xe0`. Codified as `cs8210-fan`, `cs8210-show` and
   `services.cs8210-fancontrol` in the nix module.

Open problems:

- **TPM not visible yet.** `tpm_tis_spi spi1.0: TPM vid 0xFFFF` and the aux SPI
  flash reads JEDEC `ff ff ff`: the whole aux SPI bus reads back ones under
  mainline. U-Boot itself prints `al_tpm_tis20_end: write failed / TPM extend
  failed` on every `booti` boot of ours, but not on the stock ONIE `bootm` path,
  where both U-Boot and the 4.9 kernel see the TPM. So the bus is already dead
  before Linux starts on our path. Suspects: `scsi init`, or `booti` vs `bootm`.
- Second NIC (`00:03.0`) needs an Alpine V3 AVG SerDes port; left down.
- MACsec: the FPGA `1d0f:ec30` and the MACsec card's LM75s/CPLD are visible; the
  Credo CMS50216 PHYs behind the FPGA MDIO are not probed yet.
- Workflow notes: no aarch64 builder was reachable, so the kernel and al_eth are
  cross-built from x86 (`hardware.cs8210.kernelBuildSystem`); `lldpd` is disabled
  on this host because it does not compile for aarch64 on current nixpkgs
  master. A temporary bring-up ssh key was appended to
  `/root/.ssh/authorized_keys` on the switch over the serial console (not in
  the nix repo).

### 2026-11-09 (evening) — SPI flash backup, live DT capture, NixOS port started

User direction: back up ONIE "in case we fuck up", then port the DMC12 NixOS
module (https://codeberg.org/dma/dmc12-nixos, Alpine v2 K2C-AB) to this Alpine v3
K2X-N board inside `nepeat/nix`, boot it, get console, install to `/dev/sda`.
Boot goals: al_eth management port, PCIe (switch ASIC), TPM, MACsec gear visible.

- **Flash backup done.** All six MTD partitions read over ssh from ONIE into
  `images/spi-2026-11-09/` (local-only). SHA-256 of every file matches
  `sha256sum /dev/mtdNro` on the device (`device-sha256.txt`). ONIE = `mtd4-onie.bin`
  `63c70b99...daba3b`; U-Boot `a22ff482...1897f6`; env `f3dfc3e9...a2a480`.
- Live evidence in `evidence/live-onie-2026-11-09/`: runtime FDT (+ decompiled),
  `/proc/iomem`, `/proc/interrupts`, `lspci -vvnn`/`-xxx`, dmesg.
  - RAM banks from U-Boot: 0-2G, 2-3G, 4-8G, 8-9G; vendor no-map: ECC poison
    `0x1e0000`/`0x1e1100`, APCEA `0xb0000000`+256M.
  - Timer 75 MHz (DT property), sbclk 500 MHz, MSI SPIs 160+800.
  - External PCIe: port0 ctrl `0xfd800000` ECAM `0xfb600000` INTx SPI41;
    port1 `0xfd820000`/`0xfb700000` SPI42; port4 `0xfd920000`/`0xfba00000` SPI45.
  - `mdio-fpga` maps `0xe0200000` = port 4's window, so **`1d0f:ec30` is the MDIO
    FPGA in front of the MACsec PHYs** (stock driver `cms50216.ko`, Credo
    CMS50216); `i2c_accel_fpga` maps `0xc82..0xc84` = port 1, `1d0f:8571`.
  - `spi_aux` (TPM + i2cfpga flash) has no interrupt in the vendor tree.
- **U-Boot prompt** (stop with `123`): has `booti`, `bootm`, `sysboot`, `pxe`,
  `bootmenu`, `source`, `ext4load`, `tftpboot`, `dhcp`, `unzip`, `saveenv`.
  U-Boot itself sits at ~`0x03ac0000-0x04000000` (relocaddr `0x03ed1000`).
- Gotcha: tio block-buffers its log file, so the autoboot prompt is not in the
  file until later output arrives. Watch the tmux pane, not the file.
- **NixOS port** lives in `nepeat/nix/nixos/module/cs8210` + `nixos/machine/cs8210`
  (flake output `cs8210`): mainline 7.3.3 + `alpine-v3-cs8210.dts` (rebuilt from
  the live tree) + one dw-spi poll-mode patch + vendored al_eth with the
  rev 4 -> 2 mapping. Kernel is cross-built from x86. Boot path is plain
  extlinux via U-Boot `sysboot` (DMC12's bootmenu builder was not needed).

### 2026-11-09 — first power-up, stock boot capture, storage enumeration

Capture: `serial-20261109-182401.txt` (local-only, see note at end). Outlet on at
18:25:15 PST; autoboot was not interrupted.

- Console baud confirmed 115200 8N1. U-Boot env agrees (`baudrate=115200`,
  `consoledev=ttyS0`). The 9600 SysCPLD-logger caveat did not apply to this port.
- Stock boot: `bootcmd=run boot_diagos; run check_boot_reason; run nos_bootcmd;
  run onie_bootcmd`. `nos_bootcmd=echo` (no NOS installed), so it falls through to
  ONIE from SPI flash. The DIAGOS on the SSD only boots when
  `onie_boot_reason=diag` (`diagos_bootcmd`: ext2load scsi 0:3 `/root/uImage` +
  `/root/celestica_cs8210-r0.dtb`). Autoboot stop is `Type 123<ENTER>`, 3 s.
- ONIE came up in **installer mode**, took DHCP 10.60.2.190 on eth0, started
  dropbear + telnetd, and began TFTP/HTTP installer discovery. I ran
  `onie-discovery-stop` (RAM-only) so nothing could auto-install over the SSD.
  The unit was left powered on at the `ONIE:/ #` prompt.
- Things ONIE did by itself that may have written the `uboot-env` flash
  partition (not verified either way): printed "Making NOS install boot mode
  sticky"; dropbear host keys exist in the env (`onie_dropbear_*_host_key`) and
  may have been generated and saved this boot. ONIE did not mount any `sda`
  partition.
- PCI: internal 2x al_eth, crypto, 8x AHCI (32 ports), RAID/crypto; external
  PCIE_0 Gen3 x4 `1d98:1b58` (Innovium, matches diag YAML), PCIE_1 Gen2 x1
  `1d0f:8571` (Amazon "memory controller"), PCIE_4 Gen1 x1 `1d0f:ec30`
  (Amazon ethernet; BAR sizing firmware-bug warning).
- Temperatures at idle in ONIE: 25-38 C across 24 hwmon sensors.

**Onboard storage, as seen electrically (all read-only probes):**

| device | evidence | contents |
| --- | --- | --- |
| M.2 SATA SSD, `sda` | ata16 only; S/N A069BCC5 = the imaged drive | DIAGOS, 7 partitions |
| SPI NOR `mx25u25635f`, 32 MiB (spi0.0) | U-Boot `SF:` + kernel m25p80 | mtd0 uboot, mtd1 uboot-env, mtd2 unused, mtd3 board_eeprom, mtd4 onie |
| SPI NOR `n25q256a`, 32 MiB (spi1.1) | kernel m25p80 | mtd5 `i2cfpga` |
| TPM 2.0 SLB9670 (spi1.0) | tpm_tis_spi | - |
| I2C EEPROMs (24c64 / 24c02, many) | at24 probes | FRU / SPD / module data |

So yes: there is onboard storage besides the SSD, namely **two 32 MiB SPI NOR
flashes** holding the whole boot chain and ONIE. They have not been dumped yet.

Not found: any second block device. The other 31 SATA ports report link down;
U-Boot prints `NAND: 0 MiB`; the DT `nand-flash` node (`annapurna-labs,al-nand`)
is `disabled`; the DT has no SD/eMMC/USB host node. Limit of this result: the
ONIE kernel has no MMC, USB or NVMe support built in and the NAND node is
disabled, so a physically populated NAND/eMMC that the vendor left unused would
be invisible here. The stock U-Boot env still carries Annapurna reference-board
NAND boot scripts, which proves nothing about population. A chip marking from
the compute card would settle it.

Note: the capture includes a full `fw_printenv`, which contains ONIE's dropbear
**private host keys**. The capture is therefore git-ignored (`/serial-*.txt`)
rather than committed.

### 2026-10-17 — Claude Code / Fable review

- Completed the requested broad review with `claude-fable-5-1`, plus a focused
  earlycon source check using the same model. The broad review's internal advisor
  timed out; Fable itself returned a completed assessment.
- Confirmed incorrect explicit-baud earlycon divisor and omitted vendor no-map
  reservations. Identified packaged 50-MHz versus logged 75-MHz timer discrepancy
  and the need for explicit temporary bootargs handoff.
- Independently checked the findings: rejected the missing DMC12 address claim
  and alleged variant-specific Marvell switch difference; did not accept the
  blanket inference that stock devices all used legacy interrupts.
- Saved [original review](reviews/2026-10-17-fable.md),
  [focused UART review](reviews/2026-10-17-fable-uart.md) and
  [triage with evidence and follow-ups](reviews/2026-10-17-triage.md).
- Documentation-only review. DTS, kernel config, init, scripts and artifacts
  remain unchanged; corrections are pending. No SSD or switch access this turn.

### 2026-10-17 — deeper offline research and first mainline build

- Found public MikroTik GPL patch with Alpine v3 HAL 2.6; recovered the exact
  revision-4-to-2 standard-Ethernet compatibility path and PCI coherency setup.
- Safely unpacked ONIE updater/FIT/rootfs as data, validated payload SHA-1 and
  FIT CRCs, and recovered platform startup/firmware installation details.
- Mapped both I2C trees and the board-variant discriminator. Recovered and
  emulated system/fan watchdog and inverted max-fan setter behavior with stubbed
  I2C, producing reproducible register traces. See [board-control.md](board-control.md).
- Recovered 16 Innovium IPD ABI/support type layouts and 26 ioctl dispatch targets
  from module DWARF/disassembly; complete command semantics remain unfinished.
- Added a compact serial-only DTS, kernel config, initramfs, build script and QEMU
  smoke test. Linux 7.2.11 reaches an interactive shell with four emulated A72
  CPUs at EL2. This does not validate the actual board or board DTB.
- Fixed the initramfs library-path symlink after the first emulator boot failed.
  Corrected uImage load/entry to 0x04000000 based on the modern ARM64 Image header;
  stock 0x04080000 assumes a different text offset. CRCs/alignment verified.
- Added U-Boot tools and QEMU to the devshell for packaging/offline validation.
- Renamed `scripts/inspect.py` to `inspect_ssd.py`: the old filename shadowed
  Python's standard-library inspect module when argparse imported dataclasses.
  The accidental import attempted read-only setup but failed because the old
  temporary raw image was already deleted; no loop or mount was created.
- No source SSD read, switch connection, flashing or external posting this turn.
  Updated findings, bounded source-search results and live checks in
  [offline-research.md](offline-research.md).

### 2026-10-17 — kernel and upstream portability review

- Validated vendor uImage CRCs, decompressed its ARM64 Image and recovered IKCONFIG.
- Compared both DTBs, module metadata, and vendor config against Linux 7.2.11.
- Inspected user's DMC12 repository at `5d02d1b3de04082aea3960cd74db0c24f5b3ed32`.
  Recorded reusable MSI, clock, PCIe and U-Boot packaging details and v2/v3 gaps.
- Recovered historical dmesg from the compressed image: 16 Cortex-A72 r0p3 CPUs,
  PSCI 0.2, roughly 7.75 GiB described RAM, NIC revision 4 handled via revision-2
  compatibility. These are stored logs, not new live hardware measurements.
- Recorded firmware handoff and load-address checks needed before a 7.x trial.
- All work stayed offline from the switch; temporary partition image removed.

### 2026-10-17 — initial SSD acquisition

- Created this device project at the user's request.
- Confirmed `/dev/sdb` model, serial, exact size, and that no partitions were mounted.
- Set the kernel block device read-only before acquiring; no writes to SSD media.
- Acquired the entire disk, including free/unpartitioned space, through SHA-256
  and `zstd -T4 -6 --check`, 20:41:01–20:45:43 UTC. No read errors and no zero
  substitution. Raw SHA-256:
  `4681fc3f7ab837ea51dea780834aa0336563f70adb361791d226b0b719fb21c2`.
- SMART autodetection failed through the USB adapter; explicit `-d sat` worked.
  SMART health reports PASSED, 74 power-on hours, 161 power cycles, zero reported
  reallocated sectors, uncorrectable errors, or interface CRC errors. No recorded
  SMART errors or self-tests. Vendor-specific attributes are not decoded reliably
  because this SSD is absent from the tool's database.
- The disk uses a DOS/MBR table with seven Linux filesystems and an extended
  partition. All seven were inventoried from the verified copy using read-only
  loop devices and `ro,noload,nosuid,nodev,noexec` mounts. No image programs ran.
- Full archive verification completed at 20:47:12 UTC. Compressed SHA-256:
  `0f046f54676b35a82e3bdd5dc184fb60b7da5a017fa4df26137505c2a7c66cd5`.
  Both raw and compressed checksums are also saved under `images/`.
- Saved original label photos and converted their HEIF content to viewable PNGs.
- Added zstd, smartmontools and e2fsprogs to the existing ai_re devshell for this
  acquisition and filesystem inspection. All project tooling ran in that shell
  after reading the repository instructions.

## next useful work

1. MACsec beyond static keys (MKA, rekey) and a fpp MTU <= 9198; see
   [macsec-400g.md](macsec-400g.md) "Not done".
2. Credo slice temperatures and the ASC10 voltage/temperature monitors.
3. Populate `/var/lib/cs8210-sdk/root` reproducibly (extract from the `onie`
   MTD on the box) instead of by hand; decide on enabling the service at boot.
4. Kernel MDIO/PHY and QSFP I2C drivers for the two FPGAs (register maps are
   in the reviews); real MACsec keying; switchdev-style offload on top.
5. Aux SPI flash gating, second NIC SerDes, quiet fans in U-Boot.

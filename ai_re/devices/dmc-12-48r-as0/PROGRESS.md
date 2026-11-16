# DMC-12-48R-AS0 serial console server

## Status
- **u01 (2026-11-15): stage3 → our U-Boot 2026.10 from SPI → standard boot (`bootflow scan`) →
  nixpkgs extlinux on sda1 → NixOS root sda2**; login ≈ 38 s, soak 3/3. Disk: `dmc12-disk-image`
  (`hardware.dmc12.bootLoader = "standard"`, installed with `reimage.py`).
- **Fans: fand** (python script retired 2026-11-15; failsafe/stop = 100 %).
- **Serial ports on the network: `consoled`** (`rust/consoled`, `boards/dmc12/console-server.nix`):
  on the box `line` (menu; `line list|info|watch|attach|log [-f]|send|expect|run PORT`), remote
  `ssh -t console@10.60.2.10 [watch] P12` (`ssh -t console@…` = the menu); one writer, others watch;
  keys tmux-style (`C-b`, `^]` alias; `CONSOLED_PREFIX`); runtime baud; 1 MiB RAM scrollback per
  port, lost on a consoled restart. P1 = cs8210, P48 = as7760 (115200), the rest 9600 8N1.
- **Kernel: `minimal`** (allnoconfig + explicit; 11 MiB Image, 2 modules loaded, smoke 16/16); `lean` is the fallback
  (`hardware.alpineV2.kernelVariant`).
- **USB interrupts** (494d188): the Pericom OHCI/EHCI INTx go through the PCIe port's IOFIC
  (`amazon,al-fic` @0xfd803000, GIC SPI 47). Before that they never fired and sustained 115200
  input lost ~62 % (`board-notes.md`, "USB/serial").
- **Metrics:** lab VictoriaMetrics job `dmc-12-48r-as0` (node_exporter :9100 with dmc12/consoled/fand textfiles), Grafana `ai-re-dmc12`.
- **Full Nix system (`profile-rebuildable`):** deploy in place with `./deploy.sh 10.60.2.10 [switch|boot]`
  (root flake at committed HEAD, so fand/consoled are in and nobody's uncommitted work is; copies only new paths), generations in the extlinux menu, rollback on
  the box `nixos-rebuild switch --rollback --no-reexec`.
  906 MiB used of 29 GiB.
- **SPI slots** (`spi-upgrade.py slots`; spec `spi-upgrade-plan.md`, journal `inventory/u01/spi-upgrade.jsonl`):
  IPXE 1 = env-origin fix (`.fimg` 929860b1) **[active]**, IPXE 0 = hardened build (squashed series
  0014-0023, a0328d81, fallback), UBOOT 0/1 =
  factory 2015.07. `switch --to ipxe0|ipxe1|factory`, `update` (writes the inactive slot), `rollback`.
  Holding `u` at stage3 = factory U-Boot (recovery prompt; it can't boot the standard disk).
- **U-Boot env persists** (`saveenv` at `=>`): redundant 2 × 64 KiB at 0xf90000/0xfa0000; which copy loaded
  shows on the boot line and in `alpine state` (never stored); recovery `env default -a; saveenv` (`u-boot-port.md`).
- **Open:** no clip read of CS0 yet (`spi-upgrade.py clipcheck`).
- **Board quirks/features at a glance:** `board-notes.md`. Route A / stage3 keys: `uart-boot.md`;
  our U-Boot (checklist, env, recovery): `u-boot-port.md`; boot chain: `boot-chain-details.md`,
  `../../alpine-v2-boot-chain.md`.
- Console: tmux `serial-dmc12`, tio on the FTDI `B0016Y5Q` (by-id path) at **115200 8N1**,
  confirmed from a readable boot. Captures go to `serial-*.txt` (git-ignored, may
  hold env/keys). The AS7760's console is cabled to **DMC-12 port 48**: read-only
  use only, and ask the AS7760 session first.
- Power: lab plug 2, `lab-power 2 …`. Plug 1 is the CS8210 (the other session's); don't touch it from here.
- Into a U-Boot: ours (`=>`): power-cycle and press a key at its countdown. Factory (`ALPINE_DB>`):
  `./stage3keys.py --plug 2 --key u` (stage3 `u` escape). Never `saveenv`/`env default` at the factory
  prompt (BOOT_APP reroute, `board-notes.md`). Commands go through `./console.py`.
- **Our U-Boot's env from Linux:** `dmc12-uboot-env print|set` (`boards/dmc12/uboot-env`; only the
  env window is writable in the Linux DT); `fw_printenv` for reading.
- Recovery ladder: `spi-recovery.md` (A/B in SPI, CS1 older al-boot, UART XMODEM, clip). Full images in
  `dumps/u01/`; the SSD is removable.
- Reference: units and versions `units.md` (serials in `units-private.txt`, ignored); images
  `ai-re-images` / `images.jsonl`; K2 uC + factory U-Boot internals `k2-uc.md`, vendor commands
  `uboot-vendor-commands.md`, factory hardware tooling `vendor-tooling.md`. Earlier Status text: `HISTORY.md`.
- Research (`research-open-items.md`): CS1 is picked by the K2 uC; CPLD = MachXO3 configured over
  I2C 4-0x10 (readback needs the owner's OK); uC firmware unreachable without SWD; TPM is SPI-only.
  TPM 2.0 works on SPI SS2 (2026-11-16). CPLD read back + decoded (`cpld.md`). Open: photos of the K2 card (flash/uC/headers).

## Identity
- Board EEPROM (ONIE TLV, 24c02 @ I2C 7-0051): **Accton / Edgecore**,
  `DMC-12-48R-AS0`, part `FU2AOS481000A`, label R02E, platform
  `K2C-AB-T2_DMC-12-48R-AS0`, ONIE V1.0.0.1, diag V1.0.3.0, made 2020-09-10.
  Serial, MAC and service tag are only in `inventory/u01/board-eeprom-private.txt`.
- I2C (factory kernel): SPD 24c64 @0-0057, pca9548 @0-0070, ds1339 RTC @2-0068,
  TPM `tpm_i2c_infineon` @3-0020, pca9546 @4-0071, lm75 @5-004a, tmp75
  @7-0048/49, board EEPROM 24c02 @7-0051. Board CPLD at bus 4 / 0x68 (raw, no driver).
  The live DT also has a SPI TPM (`infineon,slb9670`), sp805 watchdog, pl061 GPIO, gpio-leds.
- Board config `DMC-12 190520a alpine_v2_k2c_ab (rc mode + sata)`. DT model
  `DMC-12 Alpine V2 K2C-AB (RC + SATA)`. SoC: Annapurna Labs Alpine V2,
  4× Cortex-A57 (MIDR 0x411fd073), 4 GiB DDR (SPD at I2C 0x57).
- PCI (U-Boot): 1c36:0001 eth, 1c36:0022 ×2 (crypto, mass storage),
  1c36:0031 AHCI, behind PCIE_0 12d8:400c bridge. 48 ports = 12× CP2108 quad
  UART on USB hubs (`ttyUSB0…47`).
- CPLD (I2C 4/0x68, read-only SMBus reads, `inventory/u01/cpld-regs.txt`): version 0x88,
  fan PWM reg 0x50 = 0x2a (step 41 ≈ 68%, U-Boot's `fan_speed_70`), fans 1–3 = 0x57
  (≈ 8700 RPM at the old tool's ×100 scale), WDT flag 0x2a = 0x01, LED 0x17 = 0x20.
- PCI (Linux): al_eth, 2× al-ssm, AHCI, and a Pericom 12d8:400c bridge to OHCI ×2 + EHCI
  (12d8:400e/400f) feeding the USB hubs. USB tree: `inventory/u01/usb-tree.txt`.
- uC on the SoC card: bootloader 1.13, app 1.35.0. Secure state **NOT-SECURE**,
  recovery disabled, nitro mode disabled, physical-presence 0. Watchdog enabled
  (10 s, reset count 0). It didn't fire after minutes in U-Boot or the
  rdinit shell.
- Interrupting autoboot prints `TPM: u-boot interrupted, extending TPM PCRs!`
  (affects that boot only).

## This unit vs the first iteration (2026-11-15, offline from the SSD image)
- **A deployed fleet unit, not new-in-box.** sda1 has operator config dated Feb 2022:
  hostname, hosts, ntp, `ser2net.conf`, `users/`, `passwd`/`shadow` (changed
  2022-02-09), ssh. Not reproduced here (private). The first iteration's notes describe
  sealed liquidation units. Its env is **stock** (`bootcmd=run dmc12_bootdisk`); the
  first unit's had been rewritten to load `/boot.scr`.
- **Older OS than the one ported.** It boots `/boot/primary` = OS **1.0.457.0**
  (Mar 2020) + kernel 1.0.99.0. `/boot/secondary` is **empty**. sda1's root `/`
  (boot fallback 3) holds a newer **1.0.526.0** (Jul 2020) + kernel 1.0.101.0 that never
  boots while primary is good. The old module's fan floor (step 9 ≈ 2000 RPM, "AS0")
  and `setphyspeed -q` exist only in 1.0.526, so the first unit ran ≥1.0.526.
  1.0.526 adds nftables, sysctl and the factory tools `changemac`, `eepromer`, `fru_gen_bin`.
  It changes `fancontrol`, `setphyspeed`, `interruptmon`, `updbootenv`, `show`, `envmon` and `bindserial`.
- **Two SPI NOR chips** (live DT: dw-apb-ssi CS0/CS1). CS0 = mtd0 = booted DMC-12
  al-boot 2.60.4. CS1 = mtd1 = generic Annapurna al-boot 2.44 (Jul 2018). Unknown
  whether the first unit had the same.
- **Power:** CPLD reg 0x11 = 0x17 → PSU2 detected, **PSU1 not detected**, 12 V
  pass-through not detected. It could just be one PSU cabled; check physically.
- **Hardware otherwise matches** the old module: CPLD at bus 4 / 0x68 is version
  **0x88** (v7+, 64 PWM steps), the USB tree is 3× hub (05e3:0608) × 4 CP2108 = P1–P48 exactly as
  `serial-ports.rules` has it, and the sensors/EEPROM sit on the same buses (factory enumeration).
- Factory burn-in log on sda1 (`/dmc12_diag_log`, serial matches the EEPROM):
  **port 30 failed one serial loopback run**, then passed two reruns (48/48).
  Treat port 30 as suspect if it misbehaves.
- sda2 (14.8 GiB) is empty apart from `lost+found` and flags "needs journal recovery".

## Versions
| part | version |
| --- | --- |
| stage2 / stage3 | `stage2_loader_k2 v2.60.3` / 2.60.4 (b9e30ac, Apr 1 2019) |
| U-Boot | `2015.07-alpine_db-2.60.4-00030-g1e9d506-HAL-00010-g3eb5ee1` |
| U-Boot env | `DMC-12_UBOOTENV 1.0.81.0@edf7fb1b…` |
| kernel uImage | `DMC-12_Kernel 1.0.99.0@5ed0fb00b` (4.19.32-al-linux-v10.2.0) |
| OS initramfs | `DMC-12_OS 1.0.457.0@5035ece96ca` |

## Interfaces and storage
- Mgmt: al_eth `eth0`, RGMII at 1 Gb/s. SFP not connected. DHCP in the rdinit shell
  gave 10.60.2.10; U-Boot's `bootp` (with vendor class) got 10.60.30.12 instead.
- SPI NOR MX25U12835F, 16 MiB (`mtd0`). Flash TOC from `flash_contents_toc_print`: A/B
  stg2, dt, stg3, apcea(+cfg), uboot, uboot-script, ipxe, bootapp(+cfg),
  eeprom ×2 at 0xfc0000/0xfc1000, env 0xfd0000, redundant env 0xfe0000.
- `mtd1`: a second 16 MiB SPI image. Almost every TOC region differs from mtd0,
  and it holds generic al-boot 2.44 (Jul 2018), U-Boot `2.44-13429-g55b29e4`, no
  DMC-12 env strings. Role unconfirmed (second chip, maybe factory/recovery).
- SSD: KINGSTON RBUSNS4180S332GJ (rev SBFK), 62533296 × 512 B. ATA security
  supported, not enabled, **not frozen**. `sda1` (15.0 GiB, ext4:
  `/boot/{primary,secondary}`, `recoverymode` flag) and `sda2` (14.8 GiB). Boot
  tries primary, then secondary, then `/`.
- U-Boot env: `uboot-printenv-private.txt` (git-ignored). Interesting vars: `bootcmd=run dmc12_bootdisk`,
  `bootdelay=5`, `$bootargsextra` hook, fan presets via CPLD (I2C bus 4, 0x68).

## Dumps (`dumps/u01/`, git-ignored; zstd per the runbook)
`dumps/u01/manifest.jsonl` + `dumps/u01/SHA256SUMS` hold raw and `.zst` hashes.
- `mtd0.bin` sha256 `b66dbca586b8189a4ea0e2241d7a07642223154ea6466d7570e384c6f4462b39`
- `mtd1.bin` sha256 `9c3a95563313cdc4ae9c9840e2f1b75cfe5ffdeec2aa7052cd61c0f860178705`
- `sda.img` sha256 `46dac8f9585ee113c13e250317f07e6724d1464c2ea8e2f479148d786feedffa`
  (32,017,047,552 B → 1.05 GB zstd)
- Also copied: `live.dtb` (+ `live.dts`), board EEPROM, SPD, each hash-checked
  against the box (`fetch_small.py`).

## Bring-up checklist (switch-porting-runbook.md, phases 0–1)
Phase 0
- [x] logged tmux/tio session, by-id path; [x] baud from a readable boot
- [x] paced input (`console.py`); [x] scripted power + power-cycle to U-Boot
- [ ] logic levels: the console is the external port on the user's cable; no internal header probed
- [x] fans with no OS: **full speed**. After reset the CPLD WDT flag 0x2a = 3 and the tachs read
  0x90/0x8f/0x8f (≈14.4k RPM) while PWM 0x50 = 0x09, i.e. the CPLD fails safe until software
  clears 0x2a. The bootcmd preamble then sets 70%.
- [x] network: mgmt on the lab LAN (DHCP); [x] recovery inventory (above; JTAG unknown)
- [x] dir 0700 + `.gitignore`
- Gate: met, except for the two open safety notes above.

Phase 1
- [x] clean cold boot: `serial-20261115-initial-boot.txt` (sha256 `9d8d47aa…`)
- [x] versions (table), board EEPROM; [ ] CPLD version, uC FRU
- [x] inventory: I2C (sysfs), PCI, USB tree, CPLD registers, live DTB
- [x] SSD identify (`hdparm -I`)
- [x] dumps: mtd0, mtd1, sda, all triple-hashed and zstd-verified
- [x] unpack offline: GPT, both ext4 (debugfs, read-only), all 4 uImages, both initramfs
- [~] secure boot: uC says NOT-SECURE; OTP unread

## Log (newest first)
- 2026-11-16 03:00: **CPLD decode complete to readable RTL** (agents; `cpld.md`, `tools/machxo3/`): ladder
  L4 lossless netlist / L5 blocks / L6 templates each rebuild cfg.bin byte-identical; L7 = 18 readable
  Verilog modules, yosys-equivalent to the netlist (1042/1042). Reads of any 0x68 register proven
  side-effect-free. Found: 0x28/0x29 = a CPLD watchdog (1.034 s ticks, expiry resets), 0x2a.b1 = sticky
  WDT-reset flag forcing full fans, 0x20 = level (not latched) status, TMP75 alerts force full fans in
  hardware, 5 "I2C masters" = stuck-bus recovery engines. Top bug: no fan failsafe on a host hang unless
  the WDT is armed (software fix possible). Design data stays in dumps/.
- 2026-11-16 01:40: **production unit p01 inventoried read-only** (`units.md` "Production units"): CPLD identical
  to u01, SSD differs; its `/run` is full (journald) and it has the USB INTx bug. Tools: `cpld-readback.py
  --emit-sh/--parse-sh` and `fw-inventory.py --busybox --no-sudo` for python-less hosts. A readback leaves
  config status bits 0/3 set (`…0109` vs `…0100`, also on u01; ISC off, logic unaffected; clear on refresh).
- 2026-11-16 02:00: **CPLD read back and decoded; firmware inventory.**
  - `cpld-readback.py` (read-only subset of the factory cpldupd flow, transparent mode): LCMXO3LF-2100C
    confirmed, two identical reads, CPLD/fans unaffected (1efd88c).
  - Decode (agent; `cpld.md`, tools in `tools/machxo3/`, f6fb8b4): cfg.bin -> frames -> Trellis config ->
    cfg.bin **byte-identical**; netlist 1082 LUT4 / 921 FF / 74 pads, soft I2C slave 0x68, register map
    matches u01; 7 bug candidates (unsynchronised SCL/reset inputs, LUT-gated latches, PWM glitches).
    Phase 2 (readable RTL + equivalence, bit semantics, pin map) running. Design data stays in dumps/.
  - `fw-inventory.py` (ac8bd75): per-unit component versions + blobs, `table`/`diff --bytes`/`drift`;
    provision.py runs it. u01: 137 components, drift = BOOT_MODE + IPXE 0/1.
- 2026-11-16 00:20: **SPI TPM** (516bf72): SLB9670 node on SS2, I2C tpm@20 dropped. Probes as TPM 2.0
  (0x15D1/0x1B), PCR reads work (all zero: nothing measures), flash sha256 unchanged across the probe
  (before/after of all four mtd partitions), smoke 16/16 on minimal.
- 2026-11-16 00:05: **minimal kernel, provisioning, research.**
  - `minimal` variant fixed (MDIO_DEVICE gone in 7.3) and checked against what lean binds on u01
    (agent; 7ab341c, neutral for lean). Booted on u01: 11.1 MiB Image vs 45.7, 20 modules vs 1506,
    -70 MiB closure; no failed units, 48 ports, nft, env. Smoke failed only on the missing
    `WATCHDOG_SYSFS` (sp805 was bound): added. Reboot from minimal works. MTD numbering differs from lean
    (spi0-tail = mtd0); everything uses `/dev/mtd-u-boot-env`/names.
  - `provision.py` (87894a5): one resumable command per new unit (20 coming), Fable review fixes in.
    Untested on hardware. Needs a free switchable plug.
  - Research agent (`research-open-items.md`, read-only on u01; its bus-5..8 scan sent 4 receive-byte
    reads to the CPLD's 0x68, no writes, fans unchanged). Folded into board-notes, vendor-tooling,
    spi-recovery. `show power` printed 0 (gawk hex): fixed, 11.9 V / 1.73 A / 21.2 W.
  - Console user takes erin@sea420-desktop; VictoriaMetrics scrapes u01 (job `dmc-12-48r-as0`).
- 2026-11-15 23:15: **serial byte loss fixed: USB INTx via the IOFIC.** Reported by the AS7760/CS8210 session:
  sustained 115200 on P1 lost ~62 % of lines in contiguous ~650-byte gaps every 100 ms. Same loss with
  consoled stopped and only P1 open (plain `cat`), so not consoled; `/proc/interrupts`: the shared
  OHCI/EHCI IRQ fired once in 72 min (DT mapped INTx to GIC SPI 41). INTx go through the x8 port's app
  interrupt group A (0xfd803000, GIC SPI 47, as in the factory DT); mainline `amazon,al-fic` + `AL_FIC`
  (494d188, rising edge). After: stream-check (`rust/consoled/tools/stream-check.py`, 2.13 MB nonstop
  through consoled with all 48 open) **0 lost**; enumeration 21 → 4 s, 48 opens 56 → 0.45 s; smoke
  16/16. Also deployed b817802 (consoled keys/send/expect/run/log -f/baud); `deploy.sh` now builds from
  committed HEAD (3d93e11) after it shipped another session's uncommitted fand edits for ~10 min.
- 2026-11-15 21:35: **consoled + env-origin fix.** U-Boot 0021 no longer stores `$env_source` (origin computed
  from gd: boot line + `alpine state`), rehearsed + `update` into IPXE 1. New Rust service `rust/consoled`
  (own workspace: fand's package source is the whole `rust/crates` workspace): daemon owning the 48 ports
  (TIOCEXCL, raw termios, poll loop; device open/close/break on a worker thread: opening the 48 CP2108
  ports took 56 s and starved clients in the first version), `console` client (ssh ForceCommand, `^]`
  escapes, `--metrics` textfile producer). OpenSSH front: `console` user, any authorized key, any port.
  Tests: 3 unit + 2 daemon-vs-pty integration. On u01: 48/48 ports open, list/attach/watch over ssh,
  arbitrary ssh commands refused; smoke 16/16 (new consoled check).
- 2026-11-15 21:10: **fand, review fixes, squashed U-Boot, hardened build in SPI.** Fable reviews (U-Boot: no
  must-fix; host: 4 must-fix) fixed and simulated (3fd0b4f, c879b77, b720366); cold boot settled two U-Boot
  questions (GPIO 22 reads back out 1, CS1 crc 3d086ac6). fand on u01 via `deploy.sh` (ddc9cbd): WDT
  handshake, PI to the floor, stop → 100 %, metrics; python script retired (7d95452, system path unchanged).
  U-Boot hardening by the agent (allowlist w/o 0x35 EQIO, address width, pinned chip; sandbox + exhaustive
  window tests), then the Alpine/DMC-12 series squashed 22 → 10 (81d6a04, binaries byte-identical; map in
  `nix/packages/uboot/README.md`). Rehearsed + `update` into IPXE 0: cold boot with no refused opcode,
  `saveenv` persisted over AC power-cycle, smoke clean, escape OK. Build lock retired (owner).
- 2026-11-15 19:40: **env from Linux + quiet disk.** `boards/dmc12/uboot-env` (492c10e): Linux-only DT overlay
  (CS0 = spi0-boot ro, u-boot-env rw, spi0-tail ro; U-Boot's DTS untouched), U-Boot's envtools (67 KiB;
  `ubootTools` would be +23 MiB), `dmc12-uboot-env` wrapper (logged, counted, 20/day before `--force`).
  Deployed with `nixos-rebuild boot` + reboot; Linux→U-Boot and U-Boot→Linux round trip; then `status`:
  CS0 outside the window == model. First wrapper run had an empty-count bug (fixed, `switch`). Idle
  writes 9.6 → 0.6 MiB per 5 min (100b3a3): interruptmon logs alarm changes once (was PSU1 every
  60 s at err), producer units `LogLevelMax=notice`. Housekeeping: test generation removed; rehearsal
  images deleted and `rehearse` now deletes its own; stale docs fixed; this Status trimmed (`HISTORY.md`).
- 2026-11-15 18:45: **SPI env works cold.** Cause (U-Boot agent, 0cc8c34, patches 0030/0031): stage3 leaves
  the SSI slave select set (CS0) and U-Boot's DW SPI driver never deselected before its first transfer, so
  the env's RDID read ff ff ff; the factory U-Boot deselects, which hid it in the chainload test. Fix:
  clear SER at init and before each transfer. Route A rehearsal passed but **can't reproduce SPI state**
  (the factory U-Boot ran first and stage3 reads its image from RAM), so the real test was `update` into
  IPXE 1: 3 cold boots print `SSI: slave select 0x1 left set by the firmware, cleared` + `Env: SPI flash,
  copy at …`; a variable saved at `=>` survived an AC power-cycle (copies alternate 0xfa0000/0xf90000).
- 2026-11-15 18:15: **u01 is a rebuildable Nix system.** New `profile-rebuildable` (nix + switcher; no nixpkgs
  source/channel/registry, `max-jobs = 0`, idle daemon, weekly GC; installer tools off: `nixos-generate-config`
  dragged in bcachefs-tools, which failed to cross-build). Closure 866 → 919 MiB (boost 17, nix ~17, AWS
  libs 6, switcher 2.7). AS7760 toplevel drvPath unchanged. `disk.nix`: first-boot `nix-store --load-db` +
  system profile (as sd-image). Reimaged u01; 528 paths registered. No-op `nixos-rebuild switch
  --target-host`: 37 s, 2.4 MiB written; a real change (marker in /etc): 5 MiB written, gen 2 in the boot
  menu; `nixos-rebuild switch --rollback --no-reexec` on the box back to gen 1 (plain `--rollback` tries to
  rebuild nixos-rebuild from `<nixpkgs>`). `nix copy --to ssh-ng://` refuses unsigned paths; `nix-copy-closure`
  (nixos-rebuild's path) doesn't.
- 2026-11-15 17:40: **slots, standard boot, SPI env.** (1) Selfinit trimmed (f5901b0, 41 KiB free in
  the slot), RAM test PASS, rehearsed and `update`d into IPXE 1 (app 0x10000110); the running copy was
  CRC-proven at `=>` (`crc32 0x08000000 0x60000`). (2) Standard boot (f9aac05): `dmc12-disk-image` =
  sda1 /boot (generic-extlinux-compatible) + sda2 root, no raw partition/factory env; reimaged u01
  (`reimage.py` reaches the factory prompt via `u`), smoke PASS, soak 3/3; chainload image drvPath
  unchanged. (3) `spi-upgrade.py slots` + `switch` (8464421, 4bfb078): live ipxe1→ipxe0→ipxe1,
  CRC-proven, escape OK. (4) SPI env (af039fd, patch 0029 write window 0xf90000+0x20000 in the blank
  BOOT_APP 1 tail): chainload RAM test saved twice, persisted over a power-cycle, CS0 outside the window
  bit-identical; rolled into IPXE 0. **Bug:** as stage3's app the env load probes SPI too early
  (`unrecognized JEDEC id bytes: ff, ff, ff`, defaults used); `sf probe` at the prompt works. Wear:
  MX25U12835F min 100,000 erase/program cycles (datasheet Rev. 1.8 §14), redundant env = 200k saves.
  Harness bugs found and fixed on the way: stale-screen matches (console.py clears history per
  command; match after a marker instead) in rehearse, reimage boot, switch.
- 2026-11-15 16:00: soak from SPI, 6/6 PASS (power, warm CPLD reboot, 30 s cold): login 37.7–40.2 s, smoke clean,
  CPLD WDT flag 0x01 and uC 50–56 °C as before (`inventory/u01/soak.jsonl`; soak.py now matches the selfinit
  banner too).
- 2026-11-15 15:30: **u01 SPI install done** (`spi-upgrade.py install --write`, 3f28637; owner: "yolo -
  apply it either way", clip read + cold PCIe gates overridden). Two independent reviews first
  (Opus + Fable): typed-blind `sf` commands and a torn-BOOT_MODE trap (good header + bad data hangs
  stage3 past its key poll) fixed in de26892; fake-flash simulation of all paths. Writes: IPXE 0
  (0x2c0000, erase 0x80000, data then header) and BOOT_MODE app 0x110 (0x340000, 0x1000), each read
  back; full CS0 re-read `161a8c6d` = model (only those regions changed). **First real cold boot of our
  U-Boot:** `x0=0x0`, `MUIO: 5 pins, 3 changed`, `GPIO: 22 out 1 (was in 1)`, `PCIe 0: link up (our
  init)`, bootflow → NixOS, login 37.7 s, smoke 15/15. `u` escape → factory prompt + `sf probe` OK, then
  ours again. Soak: the entry above.
- 2026-11-15 14:50: `spi-upgrade.py` (189696e; spec `spi-upgrade-plan.md`), first runs on u01, zero
  SPI writes. Selfinit build with GPIO 22 (683d033, stage3app `.fimg` sha256 `2c00d920…`, 3884 B
  left in the 512 KiB IPXE slot): chainload RAM test PASS, `GPIO:  22 out 1 (was out 1)`, readback
  `dir 40 data cf` = factory (`serial-chainload-20261115-143753.txt`). `status`: CS0 crc32 08145ce6 ==
  model, OTP secure boot off, IPXE 0/1 + BOOT_MODE blank, factory `obj_validate UBOOT 0` OK (fixed: the
  factory prints `MX25U12835F` in caps). `rehearse`: RAM image with IPXE 0 + 0x40-B BOOT_MODE (app 0x110)
  → our U-Boot from stage3 (x0=0x0) → NixOS, smoke 15/15; same image with `u` → "User request for
  loading U-Boot" + factory 2015.07. Record: `inventory/u01/spi-upgrade.jsonl`. Found: the BOOT_MODE TOC
  slot is 64 KiB (docs said 4 KiB; only its first sector is written). GPIO 22 was already out 1 on the
  stage3-app path too, but the factory U-Boot ran earlier in that power-on (Route A), so cold is untested.
- Older entries (2026-11-15 up to 13:00: first boot, chainload image, dumps, factory tooling): `HISTORY.md`.

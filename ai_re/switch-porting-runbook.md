# Runbook: porting a switch to mainline Linux, NixOS, upstream U-Boot and an open switch SDK

For agents and the owner. Distilled from the AS7760-32X port (NXP T2080 ppc64 BE, BCM56930
Tomahawk-class), with lessons from the DMC-12 (boot flash A/B, consoled) and the CS8210 where named.
Work top to bottom; each phase ends in a **gate**: don't start the next phase's risky items until the
gate holds. Items marked *(not yet done on AS7760)* are aspirational: nobody has proven them on real
hardware yet. "how" sub-bullets give the technique that worked. Owner's outline:
Mapping = phases 0–1, Dev = phases 2–5, Bonus = phase 7 + the Bonus section.

Read first: the device's `PROGRESS.md` (Status block), `ai_re/CLAUDE.md`, `ai_re/nix/README.md`.

---

## Phase 0: Safety and access
Goal: a logged console, scripted power, a written recovery story, and private data fenced off.

- [ ] One serial session in a named tmux pane running tio, logged to a .txt file in the device dir
  (.log is gitignored). Use the stable `/dev/serial/by-id/` path. Every tool shares this pane; never open the UART separately.
  - how: if the user lacks the dialout group, keep one persistent sudo tio. Add tmux `pipe-pane` to a capture file that helper scripts can tail.
- [ ] Or a console-server port: the DMC-12's consoled (`rust/consoled/README.md`; `ssh -t console@<dmc12> watch|attach <label>`,
  scripts use `send`/`expect`/`run`, one writer, others watch). Its scrollback lives in RAM and is lost when consoled restarts, so
  record captures yourself: `ssh console@<dmc12> log <label> --follow > <device>/serial-<date>.txt`.
  - how: prove a new console path loses no bytes (`rust/consoled/tools/stream-check.py`) before trusting its captures.
- [ ] The console stays with the device for the whole port. If it moves, stop bootloader work and any reboot that might need a prompt until a replacement is live (the AS7760's cable moved to another project on 2026-11-15).
- [ ] Confirm baud/framing from a readable boot, then record it in PROGRESS. Never assume it.
- [ ] Paced-input helper: send keystrokes one at a time (a few ms apart). Old loaders drop characters from bursts, especially while echoing or computing CRCs.
  - how: a script returning does not mean the device command finished. Wait for the prompt regex.
- [ ] Scriptable power (smart outlet + CLI). Add a power-cycle-to-bootloader helper that watches the capture for the
  autoboot string and sends the stop key. It must handle both the factory prompt and the new loader's prompt.
  - e.g. on the AS7760: the factory loader wants "123" + Enter; upstream U-Boot wants any key.
- [ ] Record logic voltage levels and the cable/adapter in use before probing any internal header.
- [ ] Find out what the fans do with no OS driver (in the bootloader, or with a kernel that lacks a fan driver). Note it as a safety assumption.
  - e.g. on the AS7760: the fan CPLD sits at full duty until told otherwise.
- [ ] Network map: what each front-panel port is cabled to, and that the management port sits on an access
  (untagged) switch port. Use a TFTP dir that serves **only public build artifacts**.
  - house-network guard: front-panel ports go only to lab loops on the same box or a separate test switch, never to the house network, until Phase 5's guard holds (AS7760: one breakout loop; CS8210 incident in the gotchas).
- [ ] Recovery inventory: which flash the board boots from, alternate bank or not, JTAG/COP header, and what
  programmer that flash needs. On the AS7760 the boot flash is parallel CFI NOR (TSOP-56), which a SOIC-8 clip can't reach.
- [ ] Before the first capture: device dir mode 0700, and a .gitignore for `dumps/`, build outputs, `*-private.txt` and raw serial captures.
- **Gate:** the console is logged, a power cycle to the prompt is scripted, the recovery path is written down, and the ignore rules are in place.

## Phase 1: Mapping (non-destructive inventory + backups)
Goal: know exactly what is there and hold verified copies of every byte before writing anything.

- [ ] Capture a clean cold boot: attach logging before power-on, don't interrupt it, save `initial-boot-<date>.txt`.
- [ ] Versions: bootloader (version, build date, vendor tag), ONIE/NOS, kernel, diag image, SDK (strings on
  the vendor bcm.user), CPLD versions, reset config word (RCW), SoC SVR/revision, DIMM part numbers.
  - how: in the bootloader use version, bdinfo, help, flinfo, mmc info. Write `printenv` output **only to a private file**: the env can hold SSH host private keys.
- [ ] Hardware inventory from the vendor OS: `/proc/mtd`, `lspci -nn`, an I2C scan per bus, block devices, USB, GPIO chips, the live DTB (`dtc -I dtb`).
- [ ] Identify the storage medium properly (SD CID/SCR/SSR, eMMC EXT_CSD, card-detect pin). Don't trust labels or banners.
- [ ] Dump every flash partition, the whole boot disk and the env, read-only, streamed off the box.
  - how: stream with nc. The source hash, the transfer hash and the saved-file reread hash must all agree before a
    .partial is renamed to final. Keep `manifest.jsonl` + `SHA256SUMS`. Also concatenate the NOR in physical order.
  - A failed read stays .partial and is logged as failed. Do not claim it.
  - Compress verified images with zstd: they're big, and a whole SSD is mostly
    empty blocks. `nice zstd -T6 -12 --long=27 img -o img.zst`, then `zstd -t`. Bound the threads: `-T0` on a
    32 GB image next to a big nix build ran the host out of memory and the OOM killer took the whole user session
    (tmux, tio, every agent) on 2026-11-15. Keep
    the raw image's sha256 in the manifest next to the `.zst` hash so a
    decompressed copy can be checked. Then remove the raw file. A `.zst` can't be mounted or seeked
    (losetup has no compression; nbdkit 1.48 has no zstd filter). For random access, convert to qcow2 with
    zstd clusters (`qemu-img convert -c -O qcow2 -o compression_type=zstd`). Serve it with `qemu-nbd --read-only` or
    `nbdkit --readonly file x.qcow2 --filter=qcow2dec` (nixpkgs builds both with zstd). `ai-re-images export-qcow2` does the conversion; `ai-re-images` also catalogs, tags and verifies dumps (`tools/README.md`).
- [ ] Unpack offline, never mounting the source: FIT/legacy multi-images (kernel, initramfs, DTB), JFFS2 (jefferson), FAT (mtools).
- [ ] Mine the vendor diag/NOS userspace for board tables: CPLD register maps, I2C device tables, fan and
  thermal routing, SDK config.bcm/rc.soc for each port mode, LED programs, pre-emphasis scripts.
  - how: symbol tables in diag tools (e.g. cpldutil/i2ctmp device tables), disassembly of the set/check functions.
- [ ] Find GPL sibling ports for board deltas: the ONIE machine dir for a sibling board, the upstream SONiC
  device dir for an x86 sibling.
- [ ] Read the secure-boot state read-only before planning a bootloader swap: fuse/ITS bit, SRK hash, SB_EN in RCW, chip variant.
- **Gate:** hash-verified dumps of all storage and the env exist, versions and inventory are in PROGRESS, and nothing has been written.

## Phase 2: Boot path (RAM first, then installed)
Goal: boot your own kernel and userspace without touching storage; only then install.

- [ ] Small static RAM root (musl/BusyBox + diag tools). TFTP it, then use volatile setenv and `bootm <kernel> <initrd> <fdt>`. No `saveenv`.
  - how: use non-overlapping staging addresses, and assert the bundle size so the loader's initrd relocation stays clear of them.
- [ ] Smoke-test target binaries under QEMU (user or system, right CPU model) before hardware: ELF class, endianness, ABI.
- [ ] Netboot the full distro: the whole closure as squashfs inside the initramfs, tmpfs/overlay on top. Target 0 failed units.
- [ ] Management Ethernet: give DHCP at least 90 s of retries, and make sure the loader passes the MAC to Linux.
- [ ] SSH access: ask the owner which keys to authorize.
- [ ] Before overwriting storage: re-hash the backup, re-check the medium's identity (CID), write, then read back and compare hashes.
- [ ] Boot from storage volatile first (load the script by hand). Then make it the default with **one** env variable
  (a vendor hook such as `nos_bootcmd`), keeping the vendor OS as the fallback. Back up the env before every `saveenv`.
- [ ] Prove rollback: a previous generation (prev/) that can be booted from the prompt, with the command written down.
- [ ] Measure storage writes (`/sys/block/*/stat`) and cap them: volatile journal, no swap, /tmp on tmpfs.
- **Gate:** unattended cold power-on and warm reboot both reach the installed system, rollback works, and the vendor OS is still bootable.

## Phase 3: Kernel and device tree
Goal: an uninstrumented mainline kernel that sees all CPUs, all RAM, the boot flash, management Ethernet and the ASIC's BARs.

- [ ] Kernel from allnoconfig plus explicit options. Check against systemd's README requirements (AUTOFS, PSI,
  loop, `BLK_DEV_WRITE_MOUNTED` for online resize2fs, SQUASHFS/OVERLAY_FS for netboot).
- [ ] Board support as a small machine-compatible patch onto an existing mainline platform, not a new one.
- [ ] Device tree: start from the vendor DTB and move to mainline bindings (DPAA pools/clocks/ports/PCS on QorIQ).
  Keep the legacy compatibles that the old loader's fixups match on (DMA LIODN injection).
- [ ] Use the normal serial console only. No earlycon or fixed early-debug UART on Book E.
- [ ] PCIe: the CPU aperture must sit outside RAM **and** equal the bootloader's outbound window/LAW.
  - how: read the loader's LAWs (or its source), then read a BAR from userspace (mmap of `resource0`) before loading any driver.
- [ ] Flaky PCI config reads? Try `pcie_aspm.policy=performance` (holds the link in L0).
- [ ] Boot flash: right CFI command set; partitions enumerate; read-hash them against the Phase 1 dumps.
- [ ] Delete vendor DT nodes that nothing answers at, so the boot log has no noise.
- [ ] Triage every dmesg warning as fixed, explained or open (e.g. PAMU, AER UR, VPD), and write it down.
- [ ] After any kernel change, rebuild and retest every image that embeds it (RAM probe, netboot, SD).
- **Gate:** clean unattended boot with all threads and RAM, a bulk transfer over management Ethernet that matches its hash, flash partitions matching the dumps, ASIC BARs readable.

## Phase 4: Platform peripherals
Goal: every board device is bound, monitored, or listed as absent or unknown. Fans fail safe.

- [ ] I2C topology in DT under the real controller. Muxes get `i2c-mux-idle-disconnect`. Drivers bind on child
  adapters, so userspace never writes mux registers or depends on bus numbers.
- [ ] Fan driver (hwmon) built from the recovered CPLD registers: read-only probe, read back every write, refuse unsafe low duty, never stop a fan.
  - how: repeat the vendor's exact write sequence from disassembly. Treat unknown registers as unknown.
- [ ] Temperature sensors bound and labelled, cross-checked against a bootloader-prompt readout.
  - how: check what a read does on odd parts: an SMBus register read writes an MCP3425 ADC's config byte, so CS8210 reads it with a raw `i2ctransfer`.
- [ ] Fan control by fand (`rust/crates/fand`, one module per fan part in `nix/modules/thermal/`): 100% on any sensor/rotor fault or on exit
  (`ExecStopPost=fand failsafe`, systemd watchdog). Floor = the hardware minimum (`raw_min`); readback and tach-follow checks; a fan
  watchdog register as `arm` and a CPLD version as `expect`; `fand check --syntax-only` at build time. Hold a guarded fixed duty until a quiet curve is measured.
  - e.g. on the AS7760: first 80% while all board sensors read ≤40 °C, 100% at 45 °C; now fand's curve from the hardware minimum (thermal.md).
  - how: put curve idle points exactly on the minimum and let the curve reach 100% before `critical` (see gotchas).
- [ ] fand failsafe/exit tests, each ending at 100% and recorded in PROGRESS: `systemctl stop fand`, `kill -9` (Restart + ExecStopPost),
  a required sensor gone or its writer stopped, a stalled rotor or failed write, a second instance (exits before touching a fan).
  Run the ones you can't cause safely offline against a fake sysfs/CPLD tree (CS8210 PROGRESS, 2026-11-15 "fan control moved to fand").
- [ ] Feed ASIC die temperature (on-die PVTMON via the SDK) into the fan policy as a required sensor: no reading = full speed.
  - e.g. on the AS7760: `openbcm-asic-temp` (attach only) → `/run/asic/temperature` → fand zone `asic`. Test it after a cold power cycle, not only after an SDK session.
  - how: one SDK process per unit (a lock file). When the NOS owns the chip its SAI writes the same file (AS7760: `SAI_OPENBCM_TEMP_FILE` in syncd, the reader restarts when syncd stops),
    and a fand sensor `grace` (AS7760: 15 s) holds the last value across the handover and writer restarts. Check that a restart costs no failsafe blip.
- [ ] Over-temperature test, with the owner's go and the console attached: the chip's own shutdown and any software one, fired at a low threshold.
  Expect it to take the host down, and make sure nothing touches the chip after it fired (AS7760, 2026-11-15: the host rebooted itself; `RestartPreventExitStatus`, thermal.md).
- [ ] Module EEPROM tool: port → mux/channel map from the vendor table, SFF-8636/SFF-8472 decode, JSON output, hot-inserted modules.
  - how: open /dev/i2c-N with `I2C_SLAVE_FORCE` next to at24. at24 binds only at boot, and "busy" is not "empty".
  - how: page-select reads as one combined transfer (`I2C_RDWR`) under a per-bus lock, so the NOS's xcvrd and the tool can't interleave (AS7760 `as7760-qsfp`).
- [ ] Optics inventory: every module in the lab (cage, form factor, vendor, part, reach/length, what it's cabled to) from the EEPROM tool into PROGRESS. Re-check it before every SDK start; the `ai-re-switch-optics` dashboard shows it live.
- [ ] QSFP reset/LPMode/presence/interrupt lines via the CPLD (AS7760: reset/presence/interrupt done; no LPMODE pin, SFF-8636 soft low power untested).
  - how: read-modify-write under one lock (one register covers 8 cages, so a whole-byte write resets 8 modules), and ask which ports are cabled before a reset (`qsfp-control.md` don'ts).
- [ ] Board identity: find where MAC/serial really live (AS7760: ONIE TLV in a NOR partition, not the declared I2C EEPROM). Make the loader pass them on.
- [ ] LEDs: system/management LEDs (CPLD) and port LEDs (ASIC LED processor program) (AS7760: the public port LED program loads at SDK init, not checked by eye; CPLD LEDs *not yet done*).
- [ ] CPLDs: map versions and reset registers read-only. Don't run vendor reset commands you haven't verified.
- [ ] RTC: it enumerates, but on the AS7760 the oscillator is disabled. Set and enable it deliberately *(not yet done on AS7760)*.
- [ ] PSU status (AS7760: CPLD#1 bits, power-good checked with one PSU unplugged). Watchdog (SoC or CPLD), TPM, GPIO/SPI wiring *(not yet done on AS7760)*.
- **Gate:** a service watches fans and temperatures and fails to full speed, and the I2C map in PROGRESS marks each device bound, absent or unknown.

## Phase 4b: Monitoring (do before changing fan policy)
Goal: the device's health is scraped and kept, so thermal and fan changes are judged on trends, not single reads.

- [ ] node-exporter on the device (shared module `monitoring-node-exporter`, port 9100, firewall opened), cross-built for the target. Collectors: hwmon, thermal_zone, cpu, loadavg, meminfo, pressure, filesystem, diskstats, netdev, netclass, systemd, textfile, time, uname; drop noisy/useless ones.
  - how: Go's node_exporter needs POWER8 and dies with SIGILL on older PowerPC; use `node-exporter-lite` there (`nix/modules/monitoring/README.md`).
- [ ] Board producer writing textfile metrics (atomic write, timeout, delete on failure, a `_last_success_timestamp` per producer), best effort:
  - optics per port: presence, info (type, vendor, part), temperature, supply voltage, and for active optics per-lane rx power / tx bias;
  - fans: controller duty in percent, per-rotor rpm and fault with front/rear labels, thermal-service failsafe state;
  - board/CPU temperatures with friendly labels (the sensors.d names);
  - ASIC die temperature (needs the SDK attached; AS7760: `switch_asic_*` from the reader or syncd);
  - PSU presence/status, CPLD/board health registers where known *(not yet done on AS7760)*;
  - load, memory, uptime, disk and NIC counters come from node-exporter itself.
  Prefer generic names (`switch_optic_temperature_celsius{port=}`, `switch_fan_rpm{rotor=}`) with board specifics in labels, so dashboards carry over between boards.
- [ ] Register a scrape target in the lab VictoriaMetrics (`ai-re-metrics target add <device> <host>:9100`, NAME = the device dir) and confirm `up == 1` and the custom series exist. Remove it when the device goes away.
- [ ] Dashboards: the shared ones (`ai-re-lab-overview`, `-switch-thermals`, `-switch-optics`, `-node`, `-sonic`) should fill from the generic names. Add board panels as JSON in
  `tools/grafana/dashboards/` (fixed uid, `$job`/`$instance` variables, no IPs or hostnames; the UI can't save). See `tools/README.md` (Grafana).
- [ ] Baseline: at least an hour (ideally a day) of temperatures and fan rpm at the current fan policy, with the ASIC idle and initialised, before lowering fan duty. Record min/avg/max in PROGRESS (`ai-re-metrics range ... --since`).
- **Gate:** the device is scraped, a baseline is recorded, and the fan policy change can be compared against it.

## Phase 5: Switch chip
Goal: front-panel ports as Linux netdevs passing frames over real cables, all built by Nix.

- [ ] Identify the chip (PCI ID, revision register), its SDK family, and the licence of each piece (kernel modules are often GPL, userspace not).
- [ ] Pick one open SDK release that dispatches this device ID. Keep kernel and user parts from the same release (KNET protocol versions differ).
- [ ] Port the kernel modules (BDE/KNET) to the current kernel. Build them with the SDK's own endian flags for the host.
  Load them with the factory module parameters (e.g. himem, maxpayload, dmasize).
- [ ] Run `sync` after copying test binaries to the box, before any risky load. After a crash, check file sizes and verify the store (`nix-store --verify --check-contents`).
- [ ] SDK userspace platform for the host arch.
  - how: rewrite hard-coded /bin/... tool paths, drop `-Werror`, add the flags new GCC needs, build only the chip family you need.
- [ ] Attach and read the revision register. A correct decode proves PIO byte order.
- [ ] **Ask the owner:** which ports are cabled, and may the vendor port map/config be read (it stays local).
- [ ] Chip init with the factory config for one port mode. Then ps, internal MAC loopback, l2 add/show, and tx from the CPU with counters checked both ways.
- [ ] CPU RX: point an L2 entry at the CPU port and check the packet watcher shows correct port/VLAN metadata.
- [ ] KNET netdevs: destroy stale netifs/filters first. RX starts only once the SDK's RX is running.
  - how: before trusting a result, check that the SDK process you launched is the one running.
- [ ] Real links, isolated: remove all ports from the default VLAN, give each test port its own VLAN with only the CPU,
  keep the other ports disabled, use an experimental ethertype (`0x88b5`), and disable the ports again afterwards.
  - how: read both module EEPROMs first, force speed with autoneg off, use SerDes diagnostics (eye/lock) to tell the cable apart from config.
- [ ] House-network guard, before any front port meets a network you care about: SDK start, stop and restart under traffic survived on a lab loop;
  ports admin-down unless declared; one leg until LACP teardown is proven; the data plane off at boot until then. **Ask the owner** first (CS8210 incident in the gotchas).
- [ ] Check the port mode's per-lane speed limits before blaming optics or adapters.
- [ ] 100G on real cables, FEC, all lanes, SDK interrupts instead of polling, throughput, soak runs *(not yet done on AS7760; pre-emphasis and the LED program come with the SDK init since 2026-11-15)*.
- [ ] Load test that is really load: a MAC-loopback broadcast storm is throttled (see gotchas). Use a traffic generator or real line-rate peers *(not yet done on AS7760)*.
- [ ] Nix packages for the SDK modules and userspace, loaded at boot from a NixOS module (AS7760: `switch/openbcm.nix`).
- **Gate:** frames cross between two front-panel netdevs over a real cable, ASIC counters match, no errors, and everything is reproducible with nix build.

## Phase 6: NixOS integration (shared ai_re/nix tree)
Goal: the board is a thin layer on shared profiles, platforms and functions, and deploys in place.

- [ ] Layers: `profile-appliance` (+ `profile-netboot-root`) → `platforms/<soc>` → `<function>/` (thermal, switch…) → `boards/<board>` → `configs/<board>`.
  Put each thing at the most general layer where it holds true. When a second board needs something, move it up instead of copying.
- [ ] Pin the exact target triple and CPU (AS7760: powerpc64 BE ELFv2, -mcpu=e6500). Never substitute LE/POWER8 packages.
- [ ] Cross-build each package for the target before adding it to a shared profile. Write trimmed variants when a
  dependency blows up (e.g. `fastfetch-tiny`). Record packages that can't be built (e.g. ubootTools via python-cryptography/Rust).
- [ ] Image builder: closure budget check, size report, fsck, ELF audit (every binary is the target arch), header/CRC checks for legacy images.
  `disallowedReferences` on build-host tools that leak in through headers or libtool files.
- [ ] Bootloader hook installs kernel/initrd/DTB plus extlinux and keeps the previous generation.
- [ ] `deploy.sh` = `nixos-rebuild --target-host`. Prove it with a real kernel change. Boxes on small flash take `profile-rebuildable` (no on-box builds); measure what a no-op and a real deploy write.
- [ ] Deploy and rollback discipline: build from committed HEAD, not the working tree (other sessions edit it); one deployer per box; know which units
  a deploy restarts (AS7760: a changed orchagent restarts syncd, links flap) and say so; `nixos-rebuild boot` + reboot for kernel or boot changes;
  have the rollback command for this box written down and tried once (`nixos-rebuild switch --rollback`, plus `--no-reexec` on `profile-rebuildable`).
- [ ] Out-of-tree kernel modules: install via kbuild `modules_install`, so nixpkgs' module signing certificate files are generated.
- [ ] Prove refactors neutral (same `.drvPath`/output hash, or `nix store diff-closures`) and record that in PROGRESS.
- [ ] Reference files with `builtins.path`. nix/ never reads devices/. Build logs and result links go to the device's `nixos-build/`.
- **Gate:** `deploy.sh` rebuilds in place and the new generation survives a cold boot. The board comes from imports, not copies.

## Phase 7: Bootloader: upstream U-Boot (owner's Bonus)
Goal: the newest upstream U-Boot with a minimal, scoped patch series, tested in RAM, then flashed by a checked script.

- [ ] Feasibility gates: secure boot not enforced, a reference board upstream, matching flash layout, sibling GPL port read, **recovery path agreed with the owner**.
- [ ] Recovery ladder written before the first write: rungs from least to most invasive (stay at the prompt and rewrite from RAM, escape key, alternate instance,
  ROM UART boot, programmer), each marked proven on hardware / from code / inferred (DMC-12 `spi-recovery.md` §7, CS8210 `uart-recovery.md` §5).
  - how: trace what really picks the boot instance and what happens on a bad checksum. "A/B in flash" is not a fallback until that trace says so.
- [ ] Where the boot chain allows it, A/B: install into a blank or inactive slot and flip one selector, keep the factory loader reachable by a key press,
  never overwrite the active copy; rollback = reset the selector (DMC-12: IPXE 0/1 + BOOT_MODE, `spi-upgrade-plan.md`; CS8210 plan: a second TOC, `boot-chain.md`).
  With a single slot (AS7760) the ladder is the RAM rollback copy plus a programmer.
- [ ] Build the stock reference board through nixpkgs `buildUBoot`. Check the image size equals the partition and the reset vector matches the vendor image. Never flash this stock build.
- [ ] Config deltas: turn off anything that writes to absent hardware (VID regulator, reference CPLD, ID EEPROM).
  Set the real SYS_CLK, a board DT, the SPD addresses and DIMM slots, and turn off dead scan targets (SATA/USB).
- [ ] One ordered patch series with a scope per patch (platform vs board). Check it applies to a pristine tree. Keep fixes
  that could go upstream separate from board hacks.
- [ ] Refactors of the series are byte-neutral: same `u-boot.bin` sha256 (or drvPath) for every affected build (`nix/packages/uboot/README.md`).
- [ ] Static tests (ELF class/entry, CPU flags, no illegal opcodes, image size, reset vector) and a QEMU boot, no board needed.
- [ ] RAM chainload test: a RAM-linked build loaded with `tftp + go` from the current loader, driven by a harness that
  requires banner + prompt, runs checks, saves a transcript and exits non-zero on failure.
  - how (e6500): skip CCSR relocation if the parent already moved it, flush L2/CPC before invalidating, drop the
    parent's TLB1 entries, pulse the secondaries' reset after the new spin table is up, report full RAM to the OS.
  - limit: a RAM run after the factory loader can't reproduce cold state that loader leaves behind (DMC-12: the SPI controller's chip select). Rehearse with the exact payload bytes, then make the first real test a write into the inactive slot.
- [ ] In the chainloaded build, verify: console, clocks, SPD sizing, MMC, I2C, Ethernet TFTP, PCIe sees the ASIC, Linux boots with all CPUs/RAM/MAC, and the vendor env can be read (`env import -c`).
- [ ] DDR: a command that computes the controller registers from SPD and diffs them against the live factory-programmed controller. Iterate to 0 meaningful diffs.
- [ ] Diff flash chip-select (IFC) timings, live vs defines: a NOR U-Boot reprograms them while running from that flash.
- [ ] Standard boot: `bootflow scan` → extlinux (`BOOTSTD_FULL`, `SUPPORT_RAW_INITRD`, bootdev/bootmeth commands). Env scripts may only test commands that are compiled in.
- [ ] Independent reviews of the patches and the flasher (a second agent or stronger model; the DMC-12 had two) before the first write. Fix the must-fixes and run the flasher against a fake flash.
- [ ] First flash from the loader's proven write path. Stage the new image and the current one in RAM and CRC both.
  `cmp.b` flash against the backup (this proves the address map), then erase, write, compare, CRC, re-protect.
- [ ] Flasher script: image checks; SoC/RCW/identity/flash-map/DIMM checks; verified console backup; RAM rollback
  copy; typed confirmation; one retry, then write the old image back; post-boot DDR self-check. `--dry-run` first.
  - how (DMC-12 `spi-upgrade.py`): a host model of the whole flash (verified dump + a journal: intent before each write, done after a matching read-back);
    every full re-read must equal it, and after a write the diff must be exactly the intended regions. Write data first and the header last, so a torn write
    leaves an invalid object. Type each destructive command, compare its echo exactly, then send Enter. Preconditions live in a gates file, and overrides are recorded with a reason.
- [ ] First boot: banner, DDR compare, `mtest`, boot. Then a warm reboot and at least 2 unattended cold cycles; a scripted soak (DMC-12: 6/6 power, warm and 30 s cold cycles) if one exists.
- [ ] Restoring the factory image via the flasher, updating U-Boot from Linux (mtd write path), a JTAG/programmer recovery rehearsal *(not yet done on AS7760)*.
- **Gate:** NOR U-Boot cold-boots the installed system unattended, again and again. Backup and restore are documented, and RAM chainload testing still works from the new loader.

## Phase 8: NOS stack (SONiC-style / SAI)
Goal: the switch stack running as native NixOS services, first on the virtual SAI, then on the ASIC.

- [ ] Answer the target questions first: ASIC, switchdev driver?, SAI family, CPU arch/endianness, upstream device dir or nearest sibling.
- [ ] Check whether a SAI binary exists for this CPU arch. If not, that is the kill criterion for ASIC programming. Rank alternatives
  (open SDK + thin SAI, smaller open driver, emulation) and record where each claim came from.
- [ ] Pin every upstream rev in one file. Build each library natively, then cross. Smoke-test on the target, because big-endian bugs only show there.
- [ ] Config ownership agreed with the owner before the first CONFIG_DB key: the host stays NixOS, Nix keys are `enforce` or `seed`, the rest
  is the user's (`config save`); conflicts are shown (`/etc/issue`, `sonic-nix-diff`), never reverted live (`devices/sonic-nixos/config-ownership.md`).
- [ ] Ports from one Nix list: front-panel aliases (`Ethernet<cage>/<sub>`), SONiC names generated from the first lane, every split listed explicitly,
  lanes and speeds checked at build time, undeclared ports admin-down. Generate the SDK config from the same list (one source of truth for breakout);
  check each global key the public base sets against what your SDK build has.

Bring-up order: each step in a VM first where the fake backend covers it, then on the chip over the lab loop, transcript saved.
- [ ] Database layer on the target (done on AS7760: swss-common + redis). TCP access to redis (failed, undiagnosed).
- [ ] sairedis with the virtual SAI (the SAI metadata generator needs its XML::Simple mode with current doxygen), then swss/orchagent: in a VM first, then on the target (AS7760: at boot since 2026-11-15).
- [ ] Thin SAI over the open SDK (ports, L2, then L3), with a fake backend so orchagent can run in a VM against it (AS7760: sai-openbcm; L2, LAG, VRF/ECMP, ACL/CoPP on the chip).
  - how: answer every capability orchagent reads at init and CRM polls, or it aborts; the fake-backend VM reproduces those switch aborts.
- [ ] syncd on the ASIC at boot: one SDK process per unit (syncd conflicts with standalone SDK tools), syncd restarts with orchagent and always starts cold (DB flush), and it feeds the fan policy's ASIC sensor.
- [ ] A way into the SDK while the NOS owns the chip (AS7760: `openbcm-diag`, a diag-shell socket inside syncd). Without it routing bugs are guesswork.
- [ ] Ports, links, counters (flex counters seeded so syncd polls from boot), LLDP across the loop.
- [ ] L2: LAG with LACP (teamd), VLAN + SVI.
- [ ] L3: router interfaces, neighbours, routes, VRFs, ECMP. Read the hardware route table after the first route.
- [ ] Routing stack (FRR + FPM, bgpcfgd and the keys it waits for), BGP, BFD, with the firewall opened on switch interfaces only (AS7760: lab loop, 2026-11-15).
- [ ] ACL and CoPP policers, measured against a flood (`devices/sonic-nixos/copp_flood.py`; AS7760: ARP still over its cap).
- [ ] Platform API (fans, thermals, PSUs, optics, syseeprom) exposed to the NOS from the Phase 4 drivers, read-only towards fan control (AS7760: pmon on the switch).
- [ ] Metrics: the `services.sonic.metrics` producer and the `ai-re-sonic` dashboard *(AS7760: VM-tested, not deployed)*.
- [ ] Recovery as the NOS's supervisors do it: kill teamd, orchagent and syncd; `config reload` with and without a saved file; a deploy that touches orchagent; a reboot.
  Each must end with every netdev back and 0 failed units (AS7760: teamd, orchagent, reload without a saved file, 2026-11-15). Warm reboot *(not yet done on AS7760)*.
- [ ] Soak: hours with routing, BFD and the pollers running, each daemon's RSS trended (`sonic.routing.malloc-trace` for a grower). The bfdd leak showed up only here.
- [ ] SONiC at boot is the end state of phase 8: the board's generation starts the stack after its guards (cages/house network, bus health), the vendor stack stays a `Conflicts=` alternative, a safe Nix base CONFIG_DB (only cabled ports up), and two reboots plus the recovery checks above pass from boot (AS7760: 2026-11-15; CS8210: 2026-11-16, `devices/cs8210-32x/sonic.md`).
- **Gate:** the stack's daemons come up on the target with the virtual SAI, and ASIC-backed ports appear only after Phase 5's gate.
- **Gate (in service):** every step above passes on the chip and the soak shows no daemon growing *(AS7760: open; the bfdd fix isn't deployed, warm reboot not built)*.

## Phase 9: Docs, commits, publishing hygiene
Goal: the next reader understands the state in ten minutes, and nothing private leaks.

- [ ] `PROGRESS.md`: a Status block (runs / reach / change / recover / open) rewritten in place, then dated outcome bullets, newest first.
  Topic files hold the details. Archive old entries to `HISTORY.md`, and rewrite `HANDOFF.md` rather than stacking onto it.
- [ ] Keep transcripts as .txt files and link them with a hash or key line. Never paste dumps into markdown.
- [ ] Record misdiagnoses where they stand: correct the old sentence and say why it was wrong.
- [ ] Before each commit, scan staged files for serials, MACs, IPs, hostnames, keys, and env hex dumps in raw
  serial captures. Commit in topic-sized chunks. Don't push without being asked.
- [ ] Never commit agent or sub-agent raw transcripts. Never rewrite or garbage-collect history (reflog expire, gc --prune, force push) without asking.
- [ ] Vendor files (SDK configs, LED microcode, SAI .debs, factory binaries) stay out of the repo. Reference them by URL or read
  them from the local extracted image. Write the facts (lane maps, register meanings) into files of your own.
- [ ] After moving files, grep the docs and scripts for old paths. Watch for side effects from `git add -N` (flakes only see tracked files).
- [ ] Tick this runbook in the device's PROGRESS Status (which gates hold, what each is short of) and update its `DEVICES.md` row in the same turn.
- [ ] A topic file's new result goes into Status and HANDOFF in the same turn ("held" in Status above a test the topic file says ran is a bug).
- [ ] New gotchas go into this file as one line (symptom → cause → fix) with a pointer to the evidence. Don't restate the details.
- **Gate:** git status is clean, the staged diff is reviewed for private data, and PROGRESS Status matches reality.

---

## Bonus extras (beyond the phases)

- [ ] Send the platform-generic U-Boot fixes upstream (e.g. DM ns16550 clock divisor, VID/PCI config guards, pre-relocation I2C for SPD) *(not yet done on AS7760)*.
- [ ] A real board target instead of patching the reference board's code *(partly: identity patch only)*.
- [ ] Bootloader static + QEMU tests wired into `nix flake check`, so every patch change runs them.
- [ ] A quiet fan curve driven by ASIC die temperature plus board sensors, validated under traffic load *(AS7760: curve done; the only load test was throttled)*.
- [ ] Hot-plug handling for optics: bind at24 on insertion, CPLD module control, and a port LED program loaded from your own file *(AS7760: CPLD control and the public LED program; no at24 hot-bind)*.
- [ ] fw_`printenv`/fw_setenv on the target (needs a trimmed u-boot-tools cross-build) *(not yet done on AS7760; DMC-12: U-Boot's envtools, 67 KiB, behind a logged, rate-limited wrapper)*.
- [ ] Send the board DT/machine match to mainline Linux *(not yet done on AS7760)*.
- [ ] Cold DDR init test without flashing (SPL run from CPC-as-SRAM). Tried: it faulted in the SPL reset path, unresolved and moot once NOR boot worked.
- [ ] Publishing checklist done for the device dir (see Phase 9) before any push.

---

## Gotchas we actually hit (symptom → cause → fix/check)

**Kernel / DT / PCIe**
- Machine check on the first MMIO read of the ASIC (insmod or a userspace BAR read) → the DT's PCIe CPU aperture
  didn't match the bootloader's LAW, and Linux doesn't program LAWs here → set the DT aperture to the loader's window. Before that, the vendor window overlapped 16 GiB of RAM.
- Blank screen after "Starting kernel" → earlycon/fixed early-debug UART mapped I/O before Book E memory setup → use the normal console only.
- DMA RX dead under an old loader → the loader injects LIODNs by matching old compatible strings → keep legacy compatibles as secondary entries.
- Intermittent PCI config reads and missing IRQs → ASPM L0s from firmware → `pcie_aspm.policy=performance`.
- NOR detected but rejected → the manufacturer ID suggests Intel, but the CFI command set is 0002 → enable AMDSTD. Don't byte-swap based on the ID.
- Boot errors from sensors/EEPROMs → reference-board leftovers in the vendor DT → remove the nodes nothing answers at.
- "RX broken unless rxhash off" → misdiagnosis: the toggle never changed the hardware path. The real cause was a short DHCP window plus tagged multicast
  from a trunk port (no 8021Q in the kernel) → read the hardware registers before keeping a workaround.
- Linux oops in early SMP setup after a chainloaded loader → secondaries were still spinning in the parent's overwritten spin loop → pulse their reset. Reset bits are per hardware thread.
- Linux saw 2 GiB instead of 16 → the loader reported its own mapping, not SPD size → fix up the memory node from SPD.

**Boot images / storage**
- "Initramfs unpacking failed: invalid magic" → an uncompressed cpio after the gzip member was not 4-byte aligned → pad, and assert in the build.
- Loader crashed during bootm → loading the kernel at 0 overwrote exception vectors while the loader still ran → stage elsewhere and let bootm move it.
- Bootargs silently cut → the loader's console line limit (256) → keep the command line short.
- First-boot resize2fs EBUSY → kernel lacked `BLK_DEV_WRITE_MOUNTED` → enable it.
- Freshly copied files zero-length after a panic → not yet written back → `sync` before risky tests, then verify the store.
- extlinux builder failed on old generations → no DTB in pre-extlinux generations → deleting them cost rollback history. Tell the owner first.

**Bootloader**
- Autoboot never stops though keys were sent "at the prompt" → tio block-buffers its `--log-file`, so a helper
  tailing the file sees the countdown only after it expired (CS8210, DMC-12) → watch `tmux capture-pane`, not the file.
- Need a vendor-OS root shell without credentials → if the bootcmd appends a variable to bootargs (DMC-12: `$bootargsextra`),
  `setenv` it to `rdinit=/bin/sh` (initramfs) or `init=/bin/sh` and boot **without saveenv**. Nothing mounts the disk, so it's a clean imaging source.
  Before power-cutting a running vendor OS, serial break + SysRq `s` then `u` (sync, remount ro) if the kernel allows it.
- Garbled console from the new U-Boot → wrong SYS_CLK default, plus an upstream DM UART clock bug (2× too high) → derive the real clock from the factory divisor.
- Intermittent silent hang after `fatload + go` but not `tftp + go` → fatload leaves dirty cache lines and start.S flash-invalidated L2 → flush before invalidate (L2 and CPC).
- Hang returning to AS0 → the parent's protected TLB entry overlapped ours (multi-hit) → drop the parent's TLB1 entries first.
- "It dies here" readings that were false → early UART markers dropped once the FIFO filled (wait for THRE), and a debug TLB entry overlapping CCSR → validate your instrumentation first.
- The env picked the wrong branch → it tested bootdev, which wasn't compiled in → make env scripts test commands that are certainly present.
- Kconfig option silently off → the defconfig's "is not set" lines beat a meta-option's defaults → set each option explicitly and check .config.
- Stock reference config would drive a VID regulator over I2C → the board doesn't have one → disable VID before running anything.
- `bootflow scan` slow → SATA/AHCI hunting for absent disks → disable them.
- Cross toolchain: bare-metal libgcc refused `-mrelocatable`, and ppc32 glibc failed to install → use the Linux-ABI cross gcc with `stdenvNoLibc`.
- arm64 chainload (DMC-12, from design + QEMU, not yet on hardware): `go` enters with the parent's MMU and caches on → wrap the
  image as a **kernel-type** uImage and `bootm` it (MMU off, EL2, x0 = DTB, like Linux), or clean and turn off the caches in `save_boot_params`.
  A kernel-tree DTB lacks the firmware's memreserves → copy them in `ft_board_setup()`. A board header's `CFG_SYS_SDRAM_BASE`
  overrides the DT's DRAM base (QEMU: relocation landed below RAM) → leave it undefined when banks come from `/memory`.
- "A/B boot flash, so a bad write is safe" turned out false (DMC-12, static analysis) → every stage loaded only its own
  instance and hung on a bad object, with no counter or watchdog to flip it; a vendor "fallback" helper only covered
  "object not in TOC" → before writing, trace what actually picks the instance and what happens on a checksum failure
  (`devices/dmc-12-48r-as0/spi-recovery.md`). Also: a "read-only" `sf probe` can write the status register.
- RAM rehearsal passed, then the cold boot from SPI read the env flash as ff ff ff → stage3 leaves the SPI chip select set, and in the RAM test the factory
  loader had already cleared it → a RAM run can't stand in for the cold path for anything touching the boot-flash controller (DMC-12 PROGRESS, 2026-11-15 18:45).
- An escape key that "always works" couldn't help → a valid BOOT_MODE header over bad data hangs stage3 past its key poll (DMC-12, review) → write data first and the header last, and re-erase at once on a read-back mismatch (`spi-upgrade-plan.md`, Failure handling).
- A flasher typed destructive `sf` commands blind (review find, DMC-12 PROGRESS 2026-11-15 15:30) → a dropped character makes another command → compare the echo exactly before Enter.

**Switch chip / optics**
- Frames went to the wrong netdev → KNET netif IDs persist across SDK restarts → destroy stale netifs before creating new ones.
- A test "passed" against stale state → a second SDK instance launched through a FIFO never started → check the PID/start time of what you're testing.
- Module EEPROM garbled → I2C muxes left their last channel selected, so two modules answered at 0x50 → `i2c-mux-idle-disconnect`.
- EEPROM tool showed no modules after a reboot → at24 had bound the addresses → use `I2C_SLAVE_FORCE`, and treat busy as present.
- No 25G link, blamed on the adapter → the chosen port mode limited those lanes to 10G → check lane speeds in the mode's config first.
- SDK build failed on NixOS → makefiles hard-code `/bin/echo`, `/usr/bin/test`, and use `-Werror` → path rewrite script, relaxed warnings.
- Driver warns that maxpayload exceeds the device's capability → module parameters didn't match the factory's → load with the vendor's values.
- Every route add succeeded and nothing was routed (`l3 defip show` empty) → the public config.bcm sets `l3_alpm_enable=2`, the SDK
  was built without ALPM: tables laid out for ALPM, routes written where the chip never looks → drop the key (or build ALPM), make
  the SAI refuse the combination, and look at the hardware route table after the first route.
- Every unicast frame to the router MAC dropped (ARP replies, pings to the switch's own IP) → SDK init leaves per-port
  V4L3/V6L3 off, and the my-station entry didn't terminate ARP → enable L3 on every front-panel port at init, station entry
  with IPv4 + IPv6 + ARP; read PORT_TAB and the RIPC4/RDBGC counters to see where frames die.
- A MAC-loopback "storm" gave only mild load → each port receives its own pause frames, and even with pause off the
  queues drop most of it (~240 Mbit/s forwarded per port) → not a worst case; use real line-rate traffic.
- The diag shell hung at `tx 64` with all ports in loopback (also with PHY loopback) → the storm filled the egress
  queue; `tx 1` returns → seed with one packet, and expect to reset the chip (`init all`) afterwards.
- LACP never selected a member on a same-switch loop → libteam refuses to aggregate with its own system MAC → give one
  end its own system MAC for the test.
- The house uplink (USW Aggregation ↔ UDM) went down while a switch hung → an SDK restart under load soft-locked CPU 0 holding rtnl, LACP stopped, and the
  ASIC likely bridged the un-bundled legs (CS8210 PROGRESS, 2026-11-11 (z)) → lab loops only until Phase 5's house-network guard holds.

**NOS stack (SONiC on NixOS)**
- No KNET netdevs after a deploy → `nixos-rebuild` restarted only the changed orchagent while syncd kept running
  (SONiC restarts swss together with syncd) → tie syncd to orchagent (`PartOf`), and make every syncd start cold
  (flush APPL/ASIC/COUNTERS DBs), or the APPLY VIEW fails and orchagent loops.
- BGP stuck in Connect, BFD never up → the NixOS firewall dropped TCP 179 and UDP 3784/3785/4784 on the switch
  interfaces → open them on every interface except management.
- Every port VLAN read back as 0 on big-endian → orchagent wrote a `u16` attribute through the `u32` union member →
  check every `.value.<member>` against the SAI header's type (swss patch 0001); test on the BE target, not only x86.
- Two SONiC ends' BFD sessions never came up → upstream bgpcfgd creates passive sessions → make one side active.
- orchagent aborted in AclOrch init and PortInitDone never came → the SAI didn't answer ACL priority ranges and other init/CRM queries → answer what
  orchagent reads; the fake-backend VM test reproduced it (sonic-nixos PROGRESS, "orchagent runs on sai-openbcm").
- bgpcfgd configured no neighbour → it waits for DEVICE_METADATA `type`, `BGP_DEVICE_GLOBAL|STATE` and Loopback0 → seed the first two from Nix (sonic-nixos PROGRESS, FRR entry).
- Default-VRF routes missing in fpmsyncd, or stuck "queued" in zebra → FRR's `dplane_fpm_nl` without sonic-frr patch 0008 / without the offload-ack patch → `nix/modules/switch/sonic/README.md` (routing).
- ACL counters stayed empty → ACL counter polling is off by default → `counterpoll acl enable` (sonic-nixos PROGRESS, ACL entry).
- bfdd grew ~2.9 MB/min in the soak → json-c 0.19 frees containers inside `assert()`, compiled out under NDEBUG → patched json-c (`devices/sonic-nixos/bfdd-leak.md`).
- A "daemon stopped" test was undone at once → the upstream unit `Upholds` it → `systemctl mask --runtime --now`.

**Hardware and thermal**
- sensors printed PWM 255 as 128% → generic display bug → trust raw sysfs and the board tool.
- Fans pinned at 100% for days → no validated policy → guarded quiet duty with fault-to-full, and ASIC temperature later.
- ASIC temperature reader worked all day, then timed out after a cold power cycle → Tomahawk's S-bus ring map is zero until the SDK's chip reset programs it, and every earlier test ran after an `init all` → program the ring map (same values as the SDK) when it is zero, and test sensors from a cold boot.
- A fan curve whose idle point is a hair above the hardware minimum ran one CPLD step louder (54% → raw 153 = 60%) → coarse duty steps round up → put idle points exactly on the minimum.
- `critical` set where the curve was still at ~55% → bang-bang between quiet and failsafe 100% → let the curve reach 100% first and keep critical as an emergency.
- The software over-temperature shutdown fired and the whole host went down (it rebooted itself) → most likely a follow-up SDK read of the powered-down chip → after it fires nothing touches the chip until a power cycle (`RestartPreventExitStatus`; AS7760 `thermal.md`).

**Build / Nix**
- A tiny cross package pulled in nss/libdrm and failed → optional features enabled for the target → a trimmed local package.
- ubootTools (fw_`printenv`) won't cross-build → python-cryptography's Rust build rejects the target → none on the target yet.
- Kernel module rejected by NixOS aggregation → copied the .ko without its .ko.cer → kbuild `modules_install`.
- Remote-builder test hung 20 min → tested the builder against its own store, so both sides waited on the same locks → test from a separate --store.
- Remote build bootstrapped a toolchain from scratch → the client lacked locally cross-built paths → copy them over first.
- Patch "ends in middle of line" → a shell helper trimmed the trailing newline → write patches byte-exact and check they apply to a pristine tree.
- Harness printed FAIL while every check passed → a regex expected one line, or the terminal wrapped a long line → test harness matchers against saved transcripts.
- Harness "passed" on old output → it matched text still on the screen from an earlier command → clear the history per command or match after a fresh marker (DMC-12 PROGRESS, 2026-11-15 17:40).
- Go node_exporter cross-built fine and died with SIGILL on the e6500 → Go's ppc64 port needs POWER8 → `node-exporter-lite` (`nix/modules/monitoring/README.md`).
- Switch closure +235 MiB of x86_64 binaries → FRR's installed headers/libtool files referenced the build machine's clippy → strip them and add `disallowedReferences` (sonic-nixos PROGRESS, FRR entry).
- `nix copy --to ssh-ng://` refused the closure → unsigned paths (nixos-rebuild's copy accepts them); on-box `--rollback` tried to rebuild nixos-rebuild from `<nixpkgs>` → add `--no-reexec` (DMC-12 PROGRESS, 2026-11-15 18:15).

**Process / environment**
- A shell that runs `pkill -f PATTERN` (or a waiter looping on `pgrep -f PATTERN`) kills or finds *itself*, because
  its own command line contains PATTERN (hit three times on 2026-11-15) → kill by PID (`pgrep -f ... | grep -v $$`),
  or match something only the target has (`pgrep -x`, a unit name with `systemctl`).
- DHCP fails right after power-on → the upstream switch port was still coming up → retry for at least 90 s. It's not a bug.
- Linux came up on a different IP → the new loader had no ethaddr, so Linux picked a random MAC → import the MAC from the vendor env.
- Binary bytes sprayed onto the console → ran head on an ELF → never cat unknown files over serial.
- A raw sub-agent transcript got committed, and the "cleanup" (reflog expire + gc) wiped repo-wide undo history → never commit transcripts, and ask before any history rewrite.
- `git add -N` run by accident to make a flake see files → undo it at once and tell the owner.
- Dead SSH sessions left behind after the box panicked → reap them before the next test.
- A deploy shipped another session's uncommitted edits for ~10 min → `deploy.sh` built the working tree → build from committed HEAD (DMC-12 PROGRESS, 2026-11-15 23:15).
- Console captures had ~650-byte gaps (most lines lost at 115200) → the console server's USB serial INTx never fired → check a console path with `stream-check.py` before trusting its captures (DMC-12 PROGRESS, 2026-11-15 23:15).
- PROGRESS.md came out empty, and the next commit deleted it → a script ran `open(p,'w').write(open(p).read())`, which truncates before reading → edit with a tool that reads first, and check `git diff --stat` before committing (CS8210 PROGRESS, 2026-11-11 (z)).

## Decision points to raise with the owner (ask, don't assume)

- Any write to boot flash, env (`saveenv`), RCW, fuses or the original storage. State the backup and the recovery path in the same message.
- Overwriting the original disk/SD, even with a verified backup. Ask "resume probing" ≠ "install".
- Power-cycling or leaving the board up in a state without fan control (new kernel without the fan driver, experimental duty below the validated value).
- Over-temperature tests (a deliberately low shutdown threshold). Say that the host may go down with the chip.
- Overriding a flasher gate (e.g. skipping the programmer read), and moving a console cable or console-server port away from a device in bring-up.
- Enabling front-panel ports or running chip init while anything is cabled. Ask which ports go where. A hung ASIC has taken down a house uplink before (on another switch).
- Reading, using, storing or publishing vendor blobs or configs (SDK configs, LED microcode, factory binaries, closed SAI).
- Which SSH keys to authorize, and any change to other hosts (remote builders, the desktop's NixOS config).
- Committing private-looking data (chassis serials, MACs in boot logs). Pushing anything. Rewriting git history.
- Changing scope: a new target board for a plan, OS choice, an SDK route (open SDK vs smaller driver vs emulation), dropping rollback generations.
- Recovery hardware: does their programmer handle this flash package in-circuit? Is there a JTAG probe for this core?

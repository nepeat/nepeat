# Runbook: porting a switch to mainline Linux, NixOS, upstream U-Boot and an open switch SDK

For agents and the owner. Distilled from the AS7760-32X port (NXP T2080 ppc64 BE, BCM56930
Tomahawk-class). Work top to bottom; each phase ends in a **gate**: don't start the next phase's
risky items until the gate holds. Items marked *(not yet done on AS7760)* are aspirational: nobody
has proven them on real hardware yet. "how" sub-bullets give the technique that worked. Owner's outline:
Mapping = phases 0–1, Dev = phases 2–5, Bonus = phase 7 + the Bonus section.

Read first: the device's `PROGRESS.md` (Status block), `ai_re/CLAUDE.md`, `ai_re/nix/README.md`.

---

## Phase 0: Safety and access
Goal: a logged console, scripted power, a written recovery story, and private data fenced off.

- [ ] One serial session in a named tmux pane running tio, logged to a .txt file in the device dir
  (.log is gitignored). Use the stable `/dev/serial/by-id/` path. Every tool shares this pane; never open the UART separately.
  - how: if the user lacks the dialout group, keep one persistent sudo tio. Add tmux `pipe-pane` to a capture file that helper scripts can tail.
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
    `nbdkit --readonly file x.qcow2 --filter=qcow2dec` (nixpkgs builds both with zstd). Tooling for that is still to do.
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
- [ ] Monitor service: 100% on any sensor/rotor fault or on exit (`ExecStopPost`). Hold a guarded quiet duty only once measured.
  - e.g. on the AS7760: 80% while all board sensors read ≤40 °C, 100% at 45 °C. Not a tuned curve.
- [ ] Feed ASIC die temperature (on-die PVTMON via the SDK) into the fan policy as a required sensor: no reading = full speed.
  - e.g. on the AS7760: `openbcm-asic-temp` (attach only) → `/run/asic/temperature` → fand zone `asic`. Test it after a cold power cycle, not only after an SDK session.
- [ ] Module EEPROM tool: port → mux/channel map from the vendor table, SFF-8636/SFF-8472 decode, JSON output, hot-inserted modules.
  - how: open /dev/i2c-N with `I2C_SLAVE_FORCE` next to at24. at24 binds only at boot, and "busy" is not "empty".
- [ ] QSFP reset/LPMode/presence/interrupt lines via the CPLD *(not yet done on AS7760)*.
- [ ] Board identity: find where MAC/serial really live (AS7760: ONIE TLV in a NOR partition, not the declared I2C EEPROM). Make the loader pass them on.
- [ ] LEDs: system/management LEDs (CPLD) and port LEDs (ASIC LED processor program) *(not yet done on AS7760)*.
- [ ] CPLDs: map versions and reset registers read-only. Don't run vendor reset commands you haven't verified.
- [ ] RTC: it enumerates, but on the AS7760 the oscillator is disabled. Set and enable it deliberately *(not yet done on AS7760)*.
- [ ] Watchdog (SoC or CPLD), PSU status, TPM, GPIO/SPI wiring *(not yet done on AS7760)*.
- **Gate:** a service watches fans and temperatures and fails to full speed, and the I2C map in PROGRESS marks each device bound, absent or unknown.

## Phase 4b: Monitoring (do before changing fan policy)
Goal: the device's health is scraped and kept, so thermal and fan changes are judged on trends, not single reads.

- [ ] node-exporter on the device (shared module `monitoring-node-exporter`, port 9100, firewall opened), cross-built for the target. Collectors: hwmon, thermal_zone, cpu, loadavg, meminfo, pressure, filesystem, diskstats, netdev, netclass, systemd, textfile, time, uname; drop noisy/useless ones.
- [ ] Board producer writing textfile metrics (atomic write, timeout, delete on failure, a `_last_success_timestamp` per producer), best effort:
  - optics per port: presence, info (type, vendor, part), temperature, supply voltage, and for active optics per-lane rx power / tx bias;
  - fans: controller duty in percent, per-rotor rpm and fault with front/rear labels, thermal-service failsafe state;
  - board/CPU temperatures with friendly labels (the sensors.d names);
  - ASIC die temperature (needs the SDK running) *(not yet done on AS7760)*;
  - PSU presence/status, CPLD/board health registers where known *(not yet done on AS7760)*;
  - load, memory, uptime, disk and NIC counters come from node-exporter itself.
  Prefer generic names (`switch_optic_temperature_celsius{port=}`, `switch_fan_rpm{rotor=}`) with board specifics in labels, so dashboards carry over between boards.
- [ ] Register a scrape target in the lab VictoriaMetrics (`ai-re-metrics target add <device> <host>:9100`) and confirm `up == 1` and the custom series exist.
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
- [ ] Check the port mode's per-lane speed limits before blaming optics or adapters.
- [ ] 100G on real cables, FEC, all lanes, pre-emphasis, the LED program, SDK interrupts instead of polling, throughput, soak runs *(not yet done on AS7760)*.
- [ ] Nix packages for the SDK modules and userspace (done). Load them at boot from a NixOS module *(not yet done on AS7760: hand-loaded)*.
- **Gate:** frames cross between two front-panel netdevs over a real cable, ASIC counters match, no errors, and everything is reproducible with nix build.

## Phase 6: NixOS integration (shared ai_re/nix tree)
Goal: the board is a thin layer on shared profiles, platforms and functions, and deploys in place.

- [ ] Layers: `profile-appliance` (+ `profile-netboot-root`) → `platforms/<soc>` → `<function>/` (thermal, switch…) → `boards/<board>` → `configs/<board>`.
  Put each thing at the most general layer where it holds true. When a second board needs something, move it up instead of copying.
- [ ] Pin the exact target triple and CPU (AS7760: powerpc64 BE ELFv2, -mcpu=e6500). Never substitute LE/POWER8 packages.
- [ ] Cross-build each package for the target before adding it to a shared profile. Write trimmed variants when a
  dependency blows up (e.g. `fastfetch-tiny`). Record packages that can't be built (e.g. ubootTools via python-cryptography/Rust).
- [ ] Image builder: closure budget check, size report, fsck, ELF audit (every binary is the target arch), header/CRC checks for legacy images.
- [ ] Bootloader hook installs kernel/initrd/DTB plus extlinux and keeps the previous generation.
- [ ] `deploy.sh` = `nixos-rebuild --target-host`. Prove it with a real kernel change.
- [ ] Out-of-tree kernel modules: install via kbuild `modules_install`, so nixpkgs' module signing certificate files are generated.
- [ ] Prove refactors neutral (same `.drvPath`/output hash, or `nix store diff-closures`) and record that in PROGRESS.
- [ ] Reference files with `builtins.path`. nix/ never reads devices/. Build logs and result links go to the device's `nixos-build/`.
- **Gate:** `deploy.sh` rebuilds in place and the new generation survives a cold boot. The board comes from imports, not copies.

## Phase 7: Bootloader: upstream U-Boot (owner's Bonus)
Goal: the newest upstream U-Boot with a minimal, scoped patch series, tested in RAM, then flashed by a checked script.

- [ ] Feasibility gates: secure boot not enforced, a reference board upstream, matching flash layout, sibling GPL port read, **recovery path agreed with the owner**.
- [ ] Build the stock reference board through nixpkgs `buildUBoot`. Check the image size equals the partition and the reset vector matches the vendor image. Never flash this stock build.
- [ ] Config deltas: turn off anything that writes to absent hardware (VID regulator, reference CPLD, ID EEPROM).
  Set the real SYS_CLK, a board DT, the SPD addresses and DIMM slots, and turn off dead scan targets (SATA/USB).
- [ ] One ordered patch series with a scope per patch (platform vs board). Check it applies to a pristine tree. Keep fixes
  that could go upstream separate from board hacks.
- [ ] Static tests (ELF class/entry, CPU flags, no illegal opcodes, image size, reset vector) and a QEMU boot, no board needed.
- [ ] RAM chainload test: a RAM-linked build loaded with `tftp + go` from the current loader, driven by a harness that
  requires banner + prompt, runs checks, saves a transcript and exits non-zero on failure.
  - how (e6500): skip CCSR relocation if the parent already moved it, flush L2/CPC before invalidating, drop the
    parent's TLB1 entries, pulse the secondaries' reset after the new spin table is up, report full RAM to the OS.
- [ ] In the chainloaded build, verify: console, clocks, SPD sizing, MMC, I2C, Ethernet TFTP, PCIe sees the ASIC, Linux boots with all CPUs/RAM/MAC, and the vendor env can be read (`env import -c`).
- [ ] DDR: a command that computes the controller registers from SPD and diffs them against the live factory-programmed controller. Iterate to 0 meaningful diffs.
- [ ] Diff flash chip-select (IFC) timings, live vs defines: a NOR U-Boot reprograms them while running from that flash.
- [ ] Standard boot: `bootflow scan` → extlinux (`BOOTSTD_FULL`, `SUPPORT_RAW_INITRD`, bootdev/bootmeth commands). Env scripts may only test commands that are compiled in.
- [ ] Independent review of the patches and the flash plan (a second agent or stronger model) before the first write.
- [ ] First flash from the loader's proven write path. Stage the new image and the current one in RAM and CRC both.
  `cmp.b` flash against the backup (this proves the address map), then erase, write, compare, CRC, re-protect.
- [ ] Flasher script: image checks; SoC/RCW/identity/flash-map/DIMM checks; verified console backup; RAM rollback
  copy; typed confirmation; one retry, then write the old image back; post-boot DDR self-check. `--dry-run` first.
- [ ] First boot: banner, DDR compare, `mtest`, boot. Then a warm reboot and at least 2 unattended cold cycles.
- [ ] Restoring the factory image via the flasher, updating U-Boot from Linux (mtd write path), a JTAG/programmer recovery rehearsal *(not yet done on AS7760)*.
- **Gate:** NOR U-Boot cold-boots the installed system unattended, again and again. Backup and restore are documented, and RAM chainload testing still works from the new loader.

## Phase 8: NOS stack (SONiC-style / SAI)
Goal: the switch stack running as native NixOS services, first on the virtual SAI, then on the ASIC.

- [ ] Answer the target questions first: ASIC, switchdev driver?, SAI family, CPU arch/endianness, upstream device dir or nearest sibling.
- [ ] Check whether a SAI binary exists for this CPU arch. If not, that is the kill criterion for ASIC programming. Rank alternatives
  (open SDK + thin SAI, smaller open driver, emulation) and record where each claim came from.
- [ ] Pin every upstream rev in one file. Build each library natively, then cross. Smoke-test on the target, because big-endian bugs only show there.
- [ ] Database layer on the target (done on AS7760: swss-common + redis). TCP access to redis (failed, undiagnosed).
- [ ] sairedis with the virtual SAI (builds; the SAI metadata generator needs its XML::Simple mode with current doxygen), then swss/orchagent *(not yet run on AS7760)*.
- [ ] Thin SAI over the open SDK (ports, L2, then L3) *(not yet done on AS7760)*.
- [ ] Platform API (fans, thermals, optics, LEDs) exposed to the NOS from the Phase 4 drivers *(not yet done on AS7760)*.
- **Gate:** the stack's daemons come up on the target with the virtual SAI, and ASIC-backed ports appear only after Phase 5's gate.

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
- **Gate:** git status is clean, the staged diff is reviewed for private data, and PROGRESS Status matches reality.

---

## Bonus extras (beyond the phases)

- [ ] Send the platform-generic U-Boot fixes upstream (e.g. DM ns16550 clock divisor, VID/PCI config guards, pre-relocation I2C for SPD) *(not yet done on AS7760)*.
- [ ] A real board target instead of patching the reference board's code *(partly: identity patch only)*.
- [ ] Bootloader static + QEMU tests wired into `nix flake check`, so every patch change runs them.
- [ ] A quiet fan curve driven by ASIC die temperature plus board sensors, validated under traffic load *(not yet done on AS7760)*.
- [ ] Hot-plug handling for optics: bind at24 on insertion, CPLD module control, and a port LED program loaded from your own file *(not yet done on AS7760)*.
- [ ] fw_`printenv`/fw_setenv on the target (needs a trimmed u-boot-tools cross-build) *(not yet done on AS7760)*.
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

**Switch chip / optics**
- Frames went to the wrong netdev → KNET netif IDs persist across SDK restarts → destroy stale netifs before creating new ones.
- A test "passed" against stale state → a second SDK instance launched through a FIFO never started → check the PID/start time of what you're testing.
- Module EEPROM garbled → I2C muxes left their last channel selected, so two modules answered at 0x50 → `i2c-mux-idle-disconnect`.
- EEPROM tool showed no modules after a reboot → at24 had bound the addresses → use `I2C_SLAVE_FORCE`, and treat busy as present.
- No 25G link, blamed on the adapter → the chosen port mode limited those lanes to 10G → check lane speeds in the mode's config first.
- SDK build failed on NixOS → makefiles hard-code `/bin/echo`, `/usr/bin/test`, and use `-Werror` → path rewrite script, relaxed warnings.
- Driver warns that maxpayload exceeds the device's capability → module parameters didn't match the factory's → load with the vendor's values.

**Hardware and thermal**
- sensors printed PWM 255 as 128% → generic display bug → trust raw sysfs and the board tool.
- Fans pinned at 100% for days → no validated policy → guarded quiet duty with fault-to-full, and ASIC temperature later.
- ASIC temperature reader worked all day, then timed out after a cold power cycle → Tomahawk's S-bus ring map is zero until the SDK's chip reset programs it, and every earlier test ran after an `init all` → program the ring map (same values as the SDK) when it is zero, and test sensors from a cold boot.
- A fan curve whose idle point is a hair above the hardware minimum ran one CPLD step louder (54% → raw 153 = 60%) → coarse duty steps round up → put idle points exactly on the minimum.
- `critical` set where the curve was still at ~55% → bang-bang between quiet and failsafe 100% → let the curve reach 100% first and keep critical as an emergency.

**Build / Nix**
- A tiny cross package pulled in nss/libdrm and failed → optional features enabled for the target → a trimmed local package.
- ubootTools (fw_`printenv`) won't cross-build → python-cryptography's Rust build rejects the target → none on the target yet.
- Kernel module rejected by NixOS aggregation → copied the .ko without its .ko.cer → kbuild `modules_install`.
- Remote-builder test hung 20 min → tested the builder against its own store, so both sides waited on the same locks → test from a separate --store.
- Remote build bootstrapped a toolchain from scratch → the client lacked locally cross-built paths → copy them over first.
- Patch "ends in middle of line" → a shell helper trimmed the trailing newline → write patches byte-exact and check they apply to a pristine tree.
- Harness printed FAIL while every check passed → a regex expected one line, or the terminal wrapped a long line → test harness matchers against saved transcripts.

**Process / environment**
- DHCP fails right after power-on → the upstream switch port was still coming up → retry for at least 90 s. It's not a bug.
- Linux came up on a different IP → the new loader had no ethaddr, so Linux picked a random MAC → import the MAC from the vendor env.
- Binary bytes sprayed onto the console → ran head on an ELF → never cat unknown files over serial.
- A raw sub-agent transcript got committed, and the "cleanup" (reflog expire + gc) wiped repo-wide undo history → never commit transcripts, and ask before any history rewrite.
- `git add -N` run by accident to make a flake see files → undo it at once and tell the owner.
- Dead SSH sessions left behind after the box panicked → reap them before the next test.

## Decision points to raise with the owner (ask, don't assume)

- Any write to boot flash, env (`saveenv`), RCW, fuses or the original storage. State the backup and the recovery path in the same message.
- Overwriting the original disk/SD, even with a verified backup. Ask "resume probing" ≠ "install".
- Power-cycling or leaving the board up in a state without fan control (new kernel without the fan driver, experimental duty below the validated value).
- Enabling front-panel ports or running chip init while anything is cabled. Ask which ports go where. A hung ASIC has taken down a house uplink before (on another switch).
- Reading, using, storing or publishing vendor blobs or configs (SDK configs, LED microcode, factory binaries, closed SAI).
- Which SSH keys to authorize, and any change to other hosts (remote builders, the desktop's NixOS config).
- Committing private-looking data (chassis serials, MACs in boot logs). Pushing anything. Rewriting git history.
- Changing scope: a new target board for a plan, OS choice, an SDK route (open SDK vs smaller driver vs emulation), dropping rollback generations.
- Recovery hardware: does their programmer handle this flash package in-circuit? Is there a JTAG probe for this core?

# AS7760 handoff (2026-11-15)

Present state only; the journal is [PROGRESS.md](PROGRESS.md), older notes
[HISTORY.md](HISTORY.md). Device: Edge-Core AS7760-32X, serial 776032X16521259,
NXP T2080 (8 e6500 threads, ppc64 big-endian), 16 GiB, BCM56930 (`14e4:b930`).

Console: DMC-12 port 48 via its consoled service (since 2026-11-15): `sudo ssh -t console@10.60.2.10 watch as7760` (read-only) or `sudo ssh -t console@10.60.2.10 as7760` (attach, write if free; `^]` then `.` quit, `w` take write, `r` release). 115200 8N1, 1 MiB scrollback replayed on attach (lost when consoled restarts); one writer, others watch.

## boot path

Boot NOR `uboot` (mtd7, 0x7F40000) = upstream U-Boot 2026.10 (`#uboot-as7760-nor`)
→ env `nos_bootcmd=bootflow scan -lb` → extlinux on SD p1 (`NIXOS_BOOT`, the last
generations with their DTBs) → NixOS 26.11 from SD p2, system
[`nixosConfigurations.as7760`](../../nix/configs/as7760/configuration.nix). ONIE in
NOR is untouched (`run onie_bootcmd` at the prompt). Up in about 2 minutes;
DHCP on eth0 can need ~30 s after power-on while the uplink port comes up.

What starts at boot: OpenBCM kmods (BDE + KNET), `fand` (fans), node-exporter
:9100 + `as7760-metrics`, and **SONiC** (`sonic.target`: redis, syncd on
sai-openbcm, orchagent, managers, FRR, pmon, LLDP). syncd owns the ASIC and
writes `/run/asic/temperature` for fand; `openbcm-asic-temp` stays down while
SONiC runs and starts again when syncd stops (`sai.restartOnStop`). One SDK
process per unit: stop `sonic.target` before running `bcm.user`. SONiC
operation, tests and debugging: [sonic-nixos PROGRESS](../sonic-nixos/PROGRESS.md).

## access

- SSH: `sudo ssh root@10.60.2.253` (root keys from the config; root@sea420-desktop deploys).
- Console: 115200 8N1, U-Boot prompt `=>`. The FTDI cable (formerly tmux
  `serial-7760`) moved to the DMC-12 project on 2026-11-15; the AS7760 console
  is now **DMC-12 port 48**, which the DMC-12 does not serve yet. Use it
  read-only and ask the DMC-12 session first. Until it is live: no U-Boot
  work, no reboots that might need a prompt.
- Power: plug 1, `lab-power 1 status|on|off` (tools/README.md, HA entity
  `switch.server_room_lab_plug`; credentials from the environment, never
  written down). Plug 2 is the DMC-12: don't touch it from here.
- Front panel: only the lab loop: a QSFP28 4x25G breakout from cage 1, leg 1
  into a QSA in cage 32 (Ethernet1/1 <-> Ethernet32/1, 25G). Nothing reaches
  the house network; keep it that way.

## deploy

`./deploy.sh [switch|boot|dry-activate]`: cross-builds
`git+file://<monorepo>?dir=ai_re#as7760` on the workstation and runs
`nixos-rebuild --target-host`; the bootloader hook refreshes extlinux on p1.
The git flake sees tracked files only (new files need `git add -N`; tell the
owner). A deploy that changes orchagent or syncd restarts syncd: SDK re-init,
links flap, fans hold through fand's 15 s sensor grace. U-Boot changes:
`./chainload_uboot.py` (RAM test), then `./flash_uboot.py` (checks, backup,
typed confirmation): both need the console.

## recovery

- System: `nixos-rebuild switch --rollback` over SSH, or an older extlinux
  entry at the console.
- No SSH: console + rescue netboot (`#as7760-netboot`, `powercycle_to_uboot.py`,
  `load_ram_probe.py`; [nixos.md](nixos.md)); written for the factory loader and
  not re-run since U-Boot moved to NOR.
- U-Boot: `flash_uboot.py --restore` (`dumps/flash-backup-20261114-pre-0013/`) from
  a working prompt; a dead NOR needs a clip/JTAG write at offset 0x7F40000
  (TSOP-56 NOR: a SOIC clip does not fit). The factory U-Boot (`dumps/mtd7.bin`)
  is kept as evidence only; flash_uboot.py refuses it.
- Originals: `dumps/` (SD image, every NOR partition, `SHA256SUMS`). The U-Boot
  env holds private key data: never print it whole; private files stay out of git.

## held

- **Over-temperature test:** `openbcm-asic-temp --shutdown-at` (default 125 °C)
  powers the core PLL down in software when no SDK `init all` armed the chip's
  own handler ([sai-openbcm README](../../nix/packages/sai-openbcm/README.md#openbcm-asic-temp)).
  Tested 2026-11-15: it fired and took the host down with it (panic=10 reboot,
  no console capture); `RestartPreventExitStatus=5` since ([thermal.md](thermal.md)).
  Open: capture the exact error with the console attached.
- **Worst-case thermal:** needs line-rate traffic; the MAC-loopback storm was
  throttled by pause frames and queue drops ([thermal.md](thermal.md)).

## background

[linux-bringup.md](linux-bringup.md) (kernel/DT fixes to keep: no earlycon, DPAA
nodes, PCIe aperture 0xc40000000, `pcie_aspm.policy=performance`, CFI 0002),
[u-boot-port.md](u-boot-port.md), [nixos.md](nixos.md), [thermal.md](thermal.md),
[qsfp-control.md](qsfp-control.md), [sonic-platform.md](sonic-platform.md),
[sd-card.md](sd-card.md), [sdk-research](sdk-research/README.md).

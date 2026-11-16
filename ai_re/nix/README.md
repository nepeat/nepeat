# ai_re/nix — shared Nix for reverse-engineered hardware

One place for everything Nix that a NixOS port to odd hardware needs, so the
next board imports instead of copy-pasting. Device directories
(`devices/<name>/`) hold notes, scripts and dumps, **no Nix**.

```
packages/   derivations: bootloaders, kernels, drivers, image builders
modules/    NixOS modules, layered from most to least reusable:
  profiles/   policy, no hardware knowledge (appliance, netboot-root, ...)
  platforms/  one SoC family: kernel, boot image format, console
  thermal/    one function, shared by boards using the same part
  monitoring/ Prometheus node metrics + textfile producers (any board)
  (switch/, ...  add a directory per function when the first module exists)
  boards/     one board = platform + functions + board deltas
configs/    complete systems (nixosConfigurations) + images derived from them
```

## using it

```sh
# from ai_re/ (works without the files being committed)
nix build path:./nix#uboot-as7760-nor
nix build path:./nix#as7760-system
nix flake show path:./nix
devices/edge-core-7760-32x/deploy.sh          # nixos-rebuild --target-host
```

From another flake or plain Nix (e.g. `nepeat/nix`):

```nix
let aiRe = import ../ai_re/nix; in {
  imports = [ aiRe.nixosModules.profile-appliance aiRe.nixosModules.board-as7760 ];
}
# aiRe.packagesFor pkgs          bootloaders etc. for a build host
# aiRe.systemsFor { nixpkgs }    { nixosConfigurations, packages }
```

The root `ai_re` flake re-exports the same `nixosModules`,
`nixosConfigurations` and `packages` (input `nixpkgs-systems`, pinned apart
from the devshell). A git flake only sees tracked files, so those exports
appear once `nix/` is committed; `path:./nix` always works.

## what is here

| layer | name | what |
| --- | --- | --- |
| profile | `profile-appliance` | small cross-built NixOS: no docs/firmware, trimmed systemd, key-only sshd, probing tools, htop + fastfetch, flakes enabled |
| profile | `profile-rebuildable` | on top of appliance: nix + switcher, so `nixos-rebuild switch --flake path:./nix#<host> --target-host root@<box>` deploys in place (copies only new paths); no nixpkgs source/channel/registry, no on-box builds (`max-jobs = 0`), idle-priority daemon, weekly GC (30 d). +53 MiB on the DMC-12. Rollback on the box: `nixos-rebuild switch --rollback --no-reexec`. Image needs a first-boot store registration (DMC-12: `disk.nix`) |
| profile | `profile-netboot-root` | whole system in the initramfs (squashfs + tmpfs), nothing on disk touched |
| platform | `platform-qoriq-t2080` | NXP T2080/e6500: allnoconfig kernel, legacy uImage, serial console; option `hardware.qoriqT2080.kernelPatches` |
| function | `thermal-as7760-fan` | Accton fan-CPLD hwmon driver, sensor labels, monitor service |
| function | `switch-sonic` | SONiC control plane as NixOS services (`services.sonic`): redis, syncd, orchagent + managers, FRR (bgpd/staticd, FPM, bgpcfgd), crash recovery, Nix/CLI config ownership; [README](modules/switch/sonic/README.md) |
| function | `switch-openbcm` | Broadcom SDK kmods at boot + /dev nodes, locked `bcm.user`, ASIC die temperature reader (`hardware.openbcm`) |
| function | `monitoring-node-exporter` | Prometheus :9100, control-plane collector set, timer-driven textfile producers; [README](modules/monitoring/README.md) |
| board | `board-as7760` (+ `-sd-install`, `-sd-persist`) | Accton AS7760-32X |
| board | `board-dmc12` | DMC-12-48R-AS0 console server (Alpine V2): verbatim import of the first iteration, not yet on the shared layers; [README](modules/boards/dmc12/README.md) |
| package | `uboot-*` | upstream U-Boot; one patch series with a per-patch `scope` in [packages/uboot/default.nix](packages/uboot/default.nix): `t208x` + `as7760` (ppc), `alpine-v2` + `dmc12` (arm64, RAM chainload from the vendor U-Boot, al_eth network + board-params handoff; `uboot-dmc12-selfinit` adds the external PCIe RC on Annapurna's PCIe HAL, MUIO/CCU setup and read-only SPI NOR, `uboot-dmc12-stage3app` wraps it as an al-boot object; [packages/uboot/alpine-v2](packages/uboot/alpine-v2/default.nix)) |
| package | `alboot` | [alboot.py](packages/alboot/alboot.py): Annapurna al-boot SPI TOC/object lister, checker and wrapper (a script, not a derivation) |
| package | `fastfetch-tiny` | fastfetch without optional libraries (cross-builds everywhere) |

**fand and `path:./nix`.** fand lives in `rust/` of the monorepo, outside this
directory. A `path:./nix` build copies only `ai_re/nix` into the store, so
`rust/` is not there to import (a relative input fails with "access to absolute
path '/nix/rust/...' is forbidden"); those builds keep the older 80% monitor.
The root flake `ai_re/flake.nix`, used as `git+file://<monorepo>?dir=ai_re` (what
`.#` inside the repo and the device `deploy.sh` do), copies the whole repository
(tracked files only, `git add -N` new ones) and passes fand in. Deploy with
`deploy.sh`, not `path:./nix`.

Monitoring: `board-as7760` enables `monitoring-node-exporter` with the
[as7760-metrics](packages/as7760-metrics/README.md) producer (`switch_*` optics,
fan and board-temperature metrics); on e6500 the exporter is
`node-exporter-lite` because Go binaries need POWER8. The DMC-12's
`boards/dmc12/monitoring.nix` runs upstream node_exporter with
[dmc12-metrics](packages/dmc12-metrics/README.md). `curl http://<switch>:9100/metrics`.

## adding a board on a platform that exists (e.g. another T2080 card)

1. `modules/boards/<board>/default.nix`: import the platform module and the
   function modules that match its parts; set board kernel patches and DT.
2. Kernel/DT deltas: `packages/linux-<platform>/boards/<board>/`.
3. U-Boot: reuse the `t208x`-scoped patches from `uboots.lib.series`, add the
   board's own (DDR, DT, identity) next to them with `scope = "<board>"`.
4. `configs/<board>/`: identity, keys, network; register it in `configs/default.nix`.
5. Export new modules in `modules/default.nix`.

## rules

- **Put a thing at the most general layer where it is true.** A fix that holds
  for the SoC goes in `platforms/`, not the board. If a second board needs
  something a board module has, move it up instead of copying it.
- Shared modules take options; they do not hard-code another board's names.
- Reference files with `builtins.path { path = ...; name = ...; }` (or
  `writeText (readFile ...)`) so editing a README does not rebuild kernels.
- Nothing here may read `devices/` (dumps and private data live there).
- Cross-built targets: check a new package builds for the *target* before
  adding it to a profile; prefer a trimmed local package over a 300 MB closure.
- Prove a refactor is a no-op: same `.drvPath` / same output hash, or
  `nix store diff-closures old new` showing only the intended change.

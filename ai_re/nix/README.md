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
`nixosConfigurations` and `packages` (`flakeOutputs` in [default.nix](default.nix);
input `nixpkgs-systems`, pinned apart from the devshell and kept on the
revision of `nix/flake.lock` by `just update`). A git flake only sees tracked
files (`git add -N` new ones; the AS7772 `deploy.sh` refuses otherwise);
`path:./nix` sees the working tree.

## what is here

| layer | name | what |
| --- | --- | --- |
| profile | `profile-appliance` | small cross-built NixOS: no docs/firmware, trimmed systemd, key-only sshd, probing tools, htop + fastfetch, flakes enabled |
| profile | `profile-rebuildable` | on top of appliance: nix + switcher, so an in-place deploy (DMC-12: its `deploy.sh`, from the root flake) deploys in place (copies only new paths); no nixpkgs source/channel/registry, no on-box builds (`max-jobs = 0`), idle-priority daemon, weekly GC (30 d). +53 MiB on the DMC-12. Rollback on the box: `nixos-rebuild switch --rollback --no-reexec`. Image needs a first-boot store registration (DMC-12: `disk.nix`) |
| profile | `profile-netboot-root` | whole system in the initramfs (squashfs + tmpfs), nothing on disk touched |
| platform | `platform-qoriq-t2080` | NXP T2080/e6500: allnoconfig kernel, legacy uImage, serial console; options `hardware.qoriqT2080.kernelPatches`, `.rust` (opt-in CONFIG_RUST) |
| platform | `platform-armada-7040` | Marvell Armada 7040 boards behind a fused vendor U-Boot: stock kernel, board DTS against the kernel's marvell dtsi, extlinux on FAT /boot for the factory `sysboot` |
| function | `thermal-as7760-fan` | Accton fan-CPLD hwmon driver (C, or Rust with `hardware.as7760Fan.driver`), sensor labels, monitor service |
| function | `switch-sonic` | SONiC control plane as NixOS services (`services.sonic`): redis, syncd, orchagent + managers, FRR (bgpd/staticd, FPM, bgpcfgd), crash recovery, Nix/CLI config ownership; [README](modules/switch/sonic/README.md) |
| function | `switch-openbcm` | Broadcom SDK kmods at boot + /dev nodes, locked `bcm.user`, ASIC die temperature reader, MDIO scripts for PHYs the SDK cannot drive; the SAI package syncd links (rust/sai, `hardware.openbcm.package` / `.rustSai`) (`hardware.openbcm`, `.mdioScript`: [sai-openbcm README](packages/sai-openbcm/README.md#openbcm-mdio-script-phys-without-an-sdk-driver)) |
| function | `monitoring-node-exporter` | Prometheus :9100, control-plane collector set, timer-driven textfile producers; [README](modules/monitoring/README.md) |
| board | `board-as7760` (+ `-sd-install`, `-sd-persist`) | Accton AS7760-32X |
| board | `board-as5114` | Edgecore AS5114-48X (Armada 7040 + A385/Aldrin2): mainline DTS, prestera FW, SPI env access; system `configs/as5114` (`as5114-sd-image`, `as5114-system`) |
| board | `board-dmc12` | DMC-12-48R-AS0 console server (Alpine V2) on `platforms/alpine-v2` + `linux-alpine`: boot paths (standard/chainload/uboot-menu), SSD image, CPLD, fans (fand from the root flake), env tool; [README](modules/boards/dmc12/README.md) |
| package | `dmc12-firmware` | installs our U-Boot into a DMC-12's IPXE slots + BOOT_MODE from Linux, payload and factory regions sha-pinned; the board module ships it (and the DT windows it writes); standalone: `nix run --refresh 'github:nepeat/nepeat?dir=ai_re#dmc12-firmware' -- status` ([packages/dmc12-firmware](packages/dmc12-firmware/default.nix)) |
| package | `uboot-*` | upstream U-Boot; one patch series with a per-patch `scope` ([packages/uboot/README.md](packages/uboot/README.md): patch table, builds, rules). ppc: `t208x` + `as7760` (`uboot-as7760-nor`, `-chainload`; + `as7772` + `fsl-sata` for `uboot-as7772-*`). arm64 Alpine V2: `alpine` + `spi-nor-ro` + `alpine-v2` + `dmc12` (RAM chainload from the vendor U-Boot; `uboot-dmc12-selfinit` adds external PCIe, MUIO/CCU setup and read-only SPI NOR, `uboot-dmc12-stage3app` wraps it as an al-boot object). arm64 Alpine V3: `alpine` + `spi-nor-ro` + `alpine-v3` + `cs8210` (`uboot-cs8210`, `-selfinit`, `-stage3app`, `-installer`) |
| package | `uboot-as5114-chainload` | Marvell Armada 7040/8040 RAM chainload U-Boot: generic `mkChainload` in [packages/uboot/armada8k](packages/uboot/armada8k/default.nix) (the TN48M's proven recipe, scopes `spi-nor-ro` + `armada8k`), first board Edgecore AS5114-48X. Never written to SPI: read-only SPI NOR, no bubt/sspi/saveenv, env nowhere, checked at build time. Written for the TN48M to adopt later; its working build in `nepeat/nix` was deliberately not migrated |
| package | `kernel-rust` | Rust in the kernel: which boards can (arm64 yes, ppc64 BE opt-in via `kernel-rust-ppc64be`), a Rust-enabled Alpine test kernel, the DMC-12 reboot module in Rust (the board's module since 2026-11-23; the C one it replaced is deleted), QEMU test against the C one's recorded behaviour, al_eth's board-parameter codec in Rust; [README](packages/kernel-rust/README.md) |
| package | `kernel-rust-ppc64be` | Rust kernel modules on 64-bit big-endian PowerPC ahead of upstream: a six-patch series on 7.3.4 (per-patch scope, platform-generic), opt-in kernel variant (`linux-qoriq-t2080 { rust = true; }`, NixOS `hardware.qoriqT2080.rust`), ABI conformance module, bindgen-vs-GCC layout check, QEMU on e6500 and POWER8 pseries; the Rust as7760-fan (`hardware.as7760Fan.driver = "rust"`) is tested there against the C one; [README](packages/kernel-rust-ppc64be/README.md) |
| package | `alboot` | [alboot.py](packages/alboot/alboot.py): Annapurna al-boot SPI TOC/object lister, checker and wrapper (a script, not a derivation); [alboot3.py](packages/alboot/alboot3.py): Alpine V3 additions (every TOC, TOC/BOOT_MODE builders) |
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

## tests: `just test`

From `ai_re/` (inside the devshell or not: the recipes go through
`nix develop -c` when pytest is not on PATH). pytest, one test per target, the
tail of the build log in the failure.

| recipe | what |
| --- | --- |
| `just test-eval` | every target evaluates to a derivation + the coverage guards; builds nothing (about 1.5 min) |
| `just test-uboot` | every `uboot-*` output (all boards, the qemu fixtures, `uboot-*-tests`) |
| `just test-packages` | the other package outputs (kernels, drivers, SDK kmods, `*-test`) and what has no flake output: SONiC packages, the Rust SAI (rust/sai, `-k sai`), libsdk, sai-teralynx, qemu-ipd, sonic_platform, `sdk-config-test` of each board |
| `just test-systems` | NixOS toplevels and images from `path:./nix` (the working tree, untracked files included) |
| `just test-deploy` | the root flake as a git flake, the form `deploy.sh` uses: tracked files only, fand/consoled from `rust/`. Fails for a file that is not `git add`ed yet |
| `just test-vm` | NixOS VM tests of the SONiC stack (kvm) |
| `just test` | all of the above + `tools/tests` |

Other recipes: `just drvpaths snapshot|diff` (neutrality proof, see the rules
below), `just update` (bumps the systems' nixpkgs in `nix/flake.lock` and the
root `flake.lock` to one revision, then `test-eval`; `just update-devshell` for
the devshell's own inputs), `just uboot-variants` (every U-Boot build with its
patch scopes).

Arguments go to pytest (`just test-uboot -k as7772`, `just test-list`);
`just nix_args="--max-jobs 2 --keep-going" test-systems` (or `AI_RE_NIX_ARGS`)
adds arguments to every `nix build`; `just jobs=3 test-packages` builds three
targets at a time. Cross builds use the configured builders as any `nix build`.

Targets are discovered, not listed: the flake outputs by `nix eval`, the rest
from the `EXTRAS` expressions in [tests/nixtargets.py](../tests/nixtargets.py)
(attribute sets are walked for derivations). The hand-kept tables are there
too: `EXTRAS`, `SKIP` (evaluated, not built), `KNOWN_BROKEN` (open bugs and
known limits: strict xfail, so a fixed one must be removed) and the coverage
claims. Guards in the eval tier fail when a `packages/<dir>`, a test-looking
`.nix` file, a U-Boot variant, a config or a board appears that nothing builds.
A new flake output needs no change; a new package without one needs an
`EXTRAS` line.

## adding a board on a platform that exists (e.g. another T2080 card)

1. `modules/boards/<board>/default.nix`: import the platform module and the
   function modules that match its parts; set board kernel patches and DT.
2. Kernel/DT deltas: `packages/linux-<platform>/boards/<board>/`.
3. U-Boot: reuse the `t208x`-scoped patches of `packages/uboot/series.nix`, add the
   board's own (DDR, DT, identity) next to them with `scope = "<board>"`; the board is
   then one `{ name, patches, config }` record in `packages/uboot/default.nix`.
4. `configs/<board>/`: identity and ports; import `configs/lab-access.nix` (root
   keys, time zone, DHCP management port); register it in `configs/default.nix`.
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
  `just drvpaths snapshot` before, `just drvpaths diff` after: every target
  `just test` builds, nothing built ([tests/drvpaths.py](../tests/drvpaths.py);
  `--explain` runs nix-diff on what moved).

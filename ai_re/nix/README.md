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
| profile | `profile-netboot-root` | whole system in the initramfs (squashfs + tmpfs), nothing on disk touched |
| platform | `platform-qoriq-t2080` | NXP T2080/e6500: allnoconfig kernel, legacy uImage, serial console; option `hardware.qoriqT2080.kernelPatches` |
| function | `thermal-as7760-fan` | Accton fan-CPLD hwmon driver, sensor labels, monitor service |
| board | `board-as7760` (+ `-sd-install`, `-sd-persist`) | Accton AS7760-32X |
| package | `uboot-*` | upstream U-Boot for T208x; patch series with a per-patch `scope` (`t208x` reusable, `as7760` board-only) in [packages/uboot/default.nix](packages/uboot/default.nix) |
| package | `fastfetch-tiny` | fastfetch without optional libraries (cross-builds everywhere) |

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

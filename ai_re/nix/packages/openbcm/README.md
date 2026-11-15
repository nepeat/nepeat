# OpenBCM (Broadcom SDK 6.5.27) for big-endian PowerPC

Source: github.com/Broadcom-Network-Switching-Software/OpenBCM at
`6d2330ce1b4fc49d681cffcf09af408661be4c62`, directory `sdk-6.5.27` (Broadcom
Switch Software License; kernel modules GPL-2). Not vendored here.

| piece | state |
| --- | --- |
| `default.nix` | pinned sparse fetch of `sdk-6.5.27`, patches applied; `kmodsFor kernel`, `bcm-user { }`, `libsdk { }` |
| `kmods.nix` | BDE + KNET modules for the target kernel. Load and pass traffic on the AS7760. |
| `bcm-user` | the diag shell, linking the whole SDK (ppc64 big-endian only so far). Attaches, inits and switches on the AS7760. |
| `libsdk { }` | the same SDK userspace as `lib/libsdk.so` (the SDK's own `BCM_COMPILE_SHARED_LIB` rule, objects built `-fPIC`), public headers under `include/openbcm/`, and the exact `-D` flags it was compiled with in `share/openbcm/cflags`. Used by [sai-openbcm](../sai-openbcm/README.md). Cross-builds; not run on hardware yet. |

```sh
nix build path:./nix#as7760-openbcm-kmods path:./nix#as7760-bcm-user
# libsdk (no flake output yet; the as7760 config only allow-lists bcm-user):
NIXPKGS_ALLOW_UNFREE=1 nix build --impure --expr 'let f = builtins.getFlake "path:'$PWD'/nix";
  s = f.nixosConfigurations.as7760; in (import '$PWD'/nix/packages/openbcm { pkgs = s.pkgs; }).libsdk { }'
```

How the source is prepared (all in `default.nix`): patch 0001 (kernel modules
on Linux 7), `fixpaths.pl` (the makefiles hard-code `/bin/echo`,
`/usr/bin/test`, ...), patch 0002 (user platform `ppc64be`), then
`Make.local.as7760` selects the chip drivers. The build takes a few minutes.

On the target the NixOS module [switch/openbcm.nix](../../modules/switch/openbcm.nix)
(`hardware.openbcm.enable`) loads the modules at boot with the factory's
`himem=0 maxpayload=128 dmasize=16M`, creates `/dev/linux-kernel-bde` (127),
`/dev/linux-user-bde` (126) and `/dev/linux-bcm-knet` (122), and puts a
`bcm.user` on PATH that takes the one-SDK-process lock
(`/run/openbcm-unit0.lock`). Only one SDK process may use a unit: two would
share the DMA pool and the S-channel. KNET receive only starts once the SDK's RX
is started (`pw start` in the diag shell). Sync after copying test binaries:
a crash before writeback leaves zero-length files.
Without `rc.soc`/`config.bcm` in the working directory it only attaches;
it does not initialise the chip or touch ports.

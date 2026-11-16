# OpenBCM (Broadcom SDK 6.5.27) for big-endian PowerPC

Source: github.com/Broadcom-Network-Switching-Software/OpenBCM at
`6d2330ce1b4fc49d681cffcf09af408661be4c62`, directory `sdk-6.5.27` (Broadcom
Switch Software License; kernel modules GPL-2). Not vendored here.

| piece | state |
| --- | --- |
| `default.nix` | pinned sparse fetch of `sdk-6.5.27`, patches applied; `kmodsFor kernel`, `bcm-user { }`, `libsdk { }` |
| `kmods.nix` | BDE + KNET modules for the target kernel. Load and pass traffic on the AS7760. |
| `bcm-user` | the diag shell, linking the whole SDK (ppc64 big-endian only so far). Attaches, inits and switches on the AS7760. |
| `configs` | `config.bcm` generators: `configs.tomahawk` makes a Tomahawk config from SONiC ports and a public reference config ([below](#configs)). Used by the AS7760 board for SAI_INIT_CONFIG_FILE. |
| `libsdk { }` | the same SDK userspace as `lib/libsdk.so` (the SDK's own `BCM_COMPILE_SHARED_LIB` rule, objects built `-fPIC`), public headers under `include/openbcm/`, and the exact `-D` flags it was compiled with in `share/openbcm/cflags`. Used by [sai-openbcm](../sai-openbcm/README.md). Cross-builds; runs on the AS7760 inside syncd (2026-11-15). |

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

## configs

`configs.tomahawk { base; label; ports; management; settings; laneKeys; }`
(`configs/default.nix`, logic in `configs/tomahawk.py`): `base` is a public
config with one 4-lane 100G port per core (the AS7760 uses sonic-buildimage's
AS7712-32X file); it gives the global keys and the per-core lane maps and
polarity and per-lane pre-emphasis. `ports` (SONiC PORT entries) become one
`portmap_<logical>=<first lane>:<Gb/s>` each, with the per-lane keys split
the way the SDK reads them. The build reads the result back with the SDK's
rules and fails unless the ports (lanes, speeds) and the effective lane maps,
polarity and pre-emphasis are the requested/base ones. Tomahawk rules
(OpenBCM 6.5.27 sources, details in the script header):

- logical port = physical + 0/1/3/5 for pipes 0-3 (allowed ranges 1-32,
  34-66, 68-100, 102-133; 0 CPU, 33/67/101/135 loopback, 134 unused;
  management 129 and 131 go to pipes 1 and 2), as the public TH configs do.
  Stable: a split never renumbers another port.
- lanes per port from the speed (100G 4, 50G 2, 40G 4 or 2, 25/10G 1); a
  core uses lane 0 if any lane; 2-lane ports start at lane 0 or 2; one VCO per
  core (no 10G next to 25G in one cage).
- every port in `pbmp_xport_xe` (else HiGig) and in `pbmp_oversubscribe`.
- polarity: a 1-lane port's value is bit 0 for its lane, a 2/4-lane port's a
  core-relative mask; the lane map is read from the port on the core's lane 0.
- pre-emphasis `serdes_preemphasis_lane<i>_<port>` per lane of the port;
  the base's (25.78G-lane) values only go to ports on that VCO (25/50/100G,
  2-lane 40G); 10G and 4-lane 40G lanes keep the SDK defaults.

Tests: `nix/modules/boards/as7760/sdk-config-test.nix`.

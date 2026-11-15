# OpenBCM (Broadcom SDK 6.5.27) for big-endian PowerPC

Source: github.com/Broadcom-Network-Switching-Software/OpenBCM at
`6d2330ce1b4fc49d681cffcf09af408661be4c62`, directory `sdk-6.5.27` (Broadcom
Switch Software License; kernel modules GPL-2). Not vendored here.

| piece | state |
| --- | --- |
| `default.nix` | pinned sparse fetch of `sdk-6.5.27`, patches applied; `kmodsFor kernel`, `bcm-user { }` |
| `kmods.nix` | BDE + KNET modules for the target kernel. Load and pass traffic on the AS7760. |
| `bcm-user` | the diag shell, linking the whole SDK (ppc64 big-endian only so far). Attaches, inits and switches on the AS7760. |

```sh
nix build path:./nix#as7760-openbcm-kmods path:./nix#as7760-bcm-user
```

How the source is prepared (all in `default.nix`): patch 0001 (kernel modules
on Linux 7), `fixpaths.pl` (the makefiles hard-code `/bin/echo`,
`/usr/bin/test`, ...), patch 0002 (user platform `ppc64be`), then
`Make.local.as7760` selects the chip drivers. The build takes a few minutes.

On the target: `insmod linux-kernel-bde.ko himem=0 maxpayload=128 dmasize=16M`
(the factory's values), then `linux-user-bde` and `linux-bcm-knet`;
`mknod /dev/linux-kernel-bde c 127 0`, `/dev/linux-user-bde c 126 0`,
`/dev/linux-bcm-knet c 122 0`. KNET receive only starts once the SDK's RX
is started (`pw start` in the diag shell). Sync after copying test binaries:
a crash before writeback leaves zero-length files.
Without `rc.soc`/`config.bcm` in the working directory it only attaches;
it does not initialise the chip or touch ports.

# OpenBCM (Broadcom SDK 6.5.27) for big-endian PowerPC

Source: github.com/Broadcom-Network-Switching-Software/OpenBCM at
`6d2330ce1b4fc49d681cffcf09af408661be4c62`, directory `sdk-6.5.27` (Broadcom
Switch Software License; kernel modules GPL-2). Not vendored here.

| piece | state |
| --- | --- |
| `kmods.nix` | Nix derivation; BDE + KNET modules for the target kernel. Load and pass traffic on the AS7760. |
| userspace `bcm.user` | Builds by hand (below), **not yet a derivation**. Attaches to the BCM56930 on the AS7760. |

Hand build of the diag shell, from a full `sdk-6.5.27` tree:

1. apply `patches/0001` (kernel modules, Linux 7);
2. `perl fixpaths.pl $(find . -name 'Makefile*' -o -name 'Make.*' -o -name '*.mk')`
   (the makefiles hard-code `/bin/echo`, `/usr/bin/test`, ...);
3. apply `patches/0002` (platform `ppc64be`);
4. `cat make/Make.local.template Make.local.as7760 > make/Make.local`;
5. `mkdir -p build/linux/user/ppc64be` (the makefile does not create it);
6. with a ppc64 BE cross GCC + perl on PATH:
   `SDK=$PWD make -C systems/linux/user/ppc64be bcm -j$(nproc)`.

Result: `build/linux/user/ppc64be/bcm.user` (about 100 MB, dynamic).
On the target: `insmod linux-kernel-bde.ko himem=0 maxpayload=128 dmasize=16M`
(the factory's values), then `linux-user-bde` and `linux-bcm-knet`;
`mknod /dev/linux-kernel-bde c 127 0`, `/dev/linux-user-bde c 126 0`,
`/dev/linux-bcm-knet c 122 0`. KNET receive only starts once the SDK's RX
is started (`pw start` in the diag shell). Sync after copying test binaries:
a crash before writeback leaves zero-length files.
Without `rc.soc`/`config.bcm` in the working directory it only attaches;
it does not initialise the chip or touch ports.

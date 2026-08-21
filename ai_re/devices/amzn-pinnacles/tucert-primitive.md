# `fastboot flash tucert` — an unauthenticated write primitive, no root needed

**Confirmed empirically on this device, 2026-08-21.** On a **locked** bootloader,
with **no root**, `fastboot flash tucert` writes **arbitrary attacker-controlled
bytes** into the IDME `t_unlock_cert` field and reports success. No signature is
checked at flash time.

This is the most promising path found so far, and unlike the Mali root exploit
it is **not something an OTA can take away** — it lives in the bootloader.

## The proof

The device was locked throughout (`ro.boot.flash.locked=1`,
`verifiedbootstate=green`):

```
$ fastboot flash tucert cert_A.bin        # 256 bytes of 0x41
Writing 'tucert'    OKAY [  0.137s]

$ adb shell cat /proc/idme/t_unlock_cert   # after reboot, unprivileged shell
AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA…
$ adb shell 'cat /proc/idme/t_unlock_cert | wc -c'
257
```

The bytes went in, survived a reboot, and read back out. Restored to zeros
afterwards; device still `green` and locked.

> **Read-back caveat that nearly hid this:** `/proc/idme` is strlen-based, so
> a field of 256 *zero* bytes and a genuinely empty field both read as 1 byte.
> The first test used zeros and looked like a no-op. Using **printable** bytes
> is what made the write visible. Don't trust "empty" from procfs.

## The asymmetry is the bug

Both unlock entry points are reachable on a locked device — they are **not**
covered by the "restricted on locked hw" allowlist that blocks everything else:

| command | 4-byte input | 256-byte input |
| --- | --- | --- |
| `flash unlock` | `signature length error, wrong signature file? do nothing!` | `unlock signature verify failed, do nothing!` |
| `flash tucert` | **`OKAY`** | **`OKAY`** |

So `unlock` is **verified at flash time** — length checked, then RSA-PSS — and
refuses to write on failure. `tucert` is **not verified at flash time at all**;
it writes whatever you give it and defers verification to boot
(`amzn_verify_temp_unlock_code`, *"Verify temp unlock cert fail, ret = %d"*).

That asymmetry hands an unauthenticated attacker a persistent, boot-area write.

### Correction to earlier notes

[unlock.md](unlock.md) and [lk-reversing.md](lk-reversing.md) said the
locked-hw allowlist was "only `getvar product`, `serialno`,
`max-download-size`". **That was wrong** — it was inferred from `getvar` and
`oem` probes only. `flash:unlock` and `flash:tucert` are also permitted, and
they are the two that matter.

## Why this is the attack surface

`t_unlock_cert` is a **1024-byte** field (per the IDME table, entry at `0x2d3c`,
value at `0x2d58`). At every boot, LK feeds that attacker-controlled buffer into
a full **LibTomCrypt ASN.1/DER + X.509** stack — the binary contains:

```
der_decode_sequence_flexi.c      der_decode_sequence_multi.c
der_decode_sequence_ex.c         der_decode_subject_public_key_info.c
der_decode_integer.c             der_decode_bit_string.c
der_decode_octet_string.c        der_decode_object_identifier.c
der_decode_printable_string.c    der_decode_ia5_string.c
der_decode_choice.c              der_decode_boolean.c
```

plus the X.509 chain handling: *"Failed to decode user certificate"*,
*"Cannot find signature in user certificate"*, *"Failed to extract root CA
public key"*, *"Failed to decode root certificate"*, *"Invalid certificate 0/1/2"*.

**`der_decode_sequence_flexi` is the notable one** — it is LibTomCrypt's
recursive, heap-allocating flexible decoder, and historically the least
hardened part of that library. Attacker-controlled DER reaching a recursive
decoder in a bootloader, parsed *before* any signature is validated, is exactly
the shape of bug worth hunting.

## Why it satisfies the "no root primitives" constraint

- **No root required** — plain `fastboot` on a locked device.
- **OTA-proof** — this is bootloader behaviour, not a kernel bug. An OTA would
  patch CVE-2022-38181 and cost us root, but would not touch this unless Amazon
  specifically fixed the tucert path. (We are also not taking OTAs.)
- **Persistent** — the write survives reboots, unlike `modprobe_path` root.
- **Repeatable and low-risk** — writes stay inside the declared 1024-byte field;
  restoring is another `flash tucert` with zeros.

## Bounds: tested, and it is NOT an overflow

Probed safely — the field after `t_unlock_cert` is `rear_cam_otp`, whose value
is the placeholder string *"rear camera otp"* rather than real calibration, and
the IDME table walk is driven by size fields rather than names, so a one-byte
overrun could only touch a name byte.

| payload | result | stored |
| --- | --- | --- |
| 256 B | `OKAY` | 256 |
| 1024 B (exact field size) | `OKAY` | 1024, all field names intact |
| **1025 B (one over)** | **`FAILED (remote: 'write tucert failed!')`** | unchanged, nothing written |

**The write is correctly bounded to the declared 1024 bytes.** There is no
buffer-overflow primitive here. Any attack has to be a **parse** bug in the
DER/X.509 handling, not a memory-safety bug at write time.

## `flash unlock` length handling — sloppy, and an anomalous success path

`flash unlock` never writes on failure, so its length handling can be probed
freely. Full sweep (raw output in
[`dumps/unlock-length-probe.txt`](dumps/unlock-length-probe.txt)):

| input size | response |
| --- | --- |
| 4 – 255 B | `signature length error, wrong signature file? do nothing!` |
| **256 – 1024 B** | `unlock signature verify failed, do nothing!` |
| **1025 B – 64 KB** | **`OKAY`** |

Two findings:

**1. The length check is a minimum, not an exact match.** Anything ≥ 256 bytes
passes the length gate and reaches RSA-PSS verification, even though an RSA-2048
signature is exactly 256 bytes. Sloppy, though LibTomCrypt's `rsa_verify_hash`
should still reject a siglen ≠ modulus length.

**2. Oversized input returns `OKAY` without verifying.** Anything past the
1024-byte field size reports success. Tested whether it actually writes, using
2048 printable bytes (`0x55`) to avoid the strlen trap — **it does not**:
`unlock_code` stays empty, `flash.locked` stays `1`, `verifiedbootstate` stays
`green`. So it is a **silent no-op that reports success**, not a write.

That is still a genuine logic bug — a handler returning success on a path that
skips both verification and the write — and the asymmetry is notable:
oversized `tucert` returns `write tucert failed!` while oversized `unlock`
returns `OKAY`. Different branches. Worth understanding when the unlock handler
is reversed, but it does not unlock anything by itself.

**The useful by-product: `flash unlock` is an oracle.** It distinguishes three
states (length error / verify failure / silent OK) with no write and no reboot
required. That is the only feedback channel found anywhere on this device, and
it makes the *unlock* path far cheaper to probe than the tucert path — no reboot
per iteration.

## The blocker: no visibility into LK

Fuzzing the DER parser through this primitive is the obvious next move, but it
would currently be **blind**:

- `oem logcat lk` and `oem dump-boot-args` are both refused on locked hardware,
  so LK's own messages — including *"Verify temp unlock cert fail, ret = %d"* —
  are unreadable.
- LK exposes **no boot property** reflecting cert state. Diffed the whole
  `ro.boot.*` set with a 1024-byte bogus cert installed against the original
  capture: no difference.
- So the only observable is coarse — does the device still reach Android — which
  detects hard crashes and nothing else. Each iteration costs a full
  flash + reboot cycle (~90 s).

**UART is the unlock for this work.** With LK console output, every malformed
cert yields a specific error and return code, turning blind fuzzing into a real
feedback loop, and `ret = %d` alone would map the parser's error paths. Finding
the UART pads is now the highest-value hardware task — see the serial workflow in
`ai_re/CLAUDE.md`.

## Also ruled out: software entry to MediaTek download mode

`adb reboot edl` does **not** drop this device into BROM or preloader USBDL — it
performs a normal reboot and comes back as `0x1949:0x0642` (ordinary ADB), not
`0e8d:0003` (BROM) or `0e8d:2000` (USBDL). So there is no software path into
download mode; the remaining BROM fuse test needs physical button combos while
powering on (VolUp, VolDown, or both), which is a hands-on task.

## Remaining next steps

1. **Get UART.** Everything else is gated on it.
2. **Then fuzz the DER parser** via `flash tucert` — malformed lengths, deep
   nesting to probe `der_decode_sequence_flexi` recursion, oversized
   INTEGER/BIT STRING headers, truncated sequences.
3. **Static-analyse `amzn_verify_temp_unlock_code`** to find the parse buffer
   and whether the cert's declared length is trusted. Blocked on the Thumb-2 /
   multi-blob problem in [lk-reversing.md](lk-reversing.md).
4. **Physical BROM probe** while the case is open for UART anyway.

## Safety notes

- Restore with `fastboot flash tucert <256 zero bytes>`; verified to work.
- A malformed cert that crashes LK is expected to be recoverable — the field can
  be rewritten from fastboot, and IDME parse failure explicitly forces fastboot
  mode rather than bricking.
- **Do not** attempt this against `flash unlock`; that path *does* verify before
  writing, so it offers no primitive and only burns reboots.

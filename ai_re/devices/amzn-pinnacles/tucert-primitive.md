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

---

## Static analysis of the verifier — UNBLOCKED 2026-08-21

Step 3 above was blocked on the "Thumb-2 / multi-blob" problem. That problem was
later solved (the image is base 0; the old scanner was broken). Re-running the
literal-resolution technique on `lk.img` locates the verifier and, more
importantly, **reveals the container format — which invalidates the fuzzing plan
in step 2 above.**

### Recovering the xrefs

Ghidra finds no references to the tucert strings (its analysis of this blob is
incomplete). They are reachable by resolving the `ldr rX,[pc,#imm]` /
`add rX,pc` pairs, where `target = *literal + addr_of_add + 4`:

| string | ldr | add |
| --- | --- | --- |
| `Failed to get temp unlock codes` | `0x01c76` | `0x01c7a` |
| `Failed to get temp unlock cert` | `0x01ea0` | `0x01ea8` |
| `Verify temp unlock cert fail, ret = %d` | `0x01f9a` | `0x01fa4` |

So the temp-unlock verifier lives at roughly **`0x1e00`–`0x2000`**.

**Method validated:** the same resolution applied to the name argument at
`0x1d10`/`0x1d14` yields the string **`t_unlock_cert`** — the exact IDME field we
already know is involved. That is the sanity check that the addressing is right,
in the same spirit as `devinfo[28] == 0x788`.

### ⚠️ The field is NOT raw DER — it is `AZTU` + base64

`idme_get` → helper at `0x1b6c` does, in order:

```
0x1bb8  bl #0x274        ; read the raw IDME field into a temp heap buffer
0x1bd4  bl #0x37bd4      ; memcmp(buf, <literal>, 4)   <-- 4-byte magic
0x1be4  bl #0x2028       ; base64-decode buf+4 into the caller's buffer
0x1bee  cmp r2, r1       ; require decoded_len == caller capacity EXACTLY
```

The magic literal resolves to **`AZTU`** (`0x480df`, bytes `41 5a 54 55`) —
presumably *AmaZon Temp Unlock*.

**This means the DER fuzzing plan in "Remaining next steps" would not have
worked as written.** Raw malformed DER written with `fastboot flash tucert`
never reaches the parser: it fails the `AZTU` memcmp first. A payload must be:

```
"AZTU" || base64( <DER bytes> )
```

and the base64 must decode to a length that exactly matches the capacity the
caller passed, or `0x1b6c` returns -1 before the parser is reached.

The temp-buffer size is computed from that capacity as
`((cap+2) * magic >> 1) << 2 + 5` — the usual 4/3 base64 expansion — so the
encoded form is sized from the expected decoded length.

### Two candidate bugs, both UNVERIFIED

Recording these as leads, not findings. Neither has been tested, and the
prologue that would settle the first one does not decode cleanly (literal pool
in the middle), so the capacity's provenance is still unknown.

**1. Undersized cert heap buffer.** The cert getter at `0x1cdc` allocates a
fixed **`0x250` = 592-byte** buffer:

```
0x1cee  mov.w r0, #0x250
0x1cf2  bl    #0x378a0     ; malloc(592)
```

while the `t_unlock_cert` IDME field is **1024 bytes**. If the decode capacity
can exceed 592, this is an attacker-controlled heap overflow reached *before any
signature check*. **But** `0x1b6c` bounds the decode by the caller-supplied
capacity, so this only bites if that capacity is wrong or uninitialised. Not
established — do not treat as a bug yet.

**2. Length underflow at `0x1f0c`.** The cert length gets only a zero-check:

```
0x1ebe  ldr.w sb, [r7, #0x14]   ; cert length
0x1ee8  cmp.w sb, #0            ; only checked against zero
0x1f0c  sub.w r1, sb, #0x100    ; len - 256  -> underflows if len < 256
0x1f18  bl    #0x19a8           ; passed straight in as a length
```

and the callee at `0x19a8` likewise only zero-checks it (`0x19da`). A cert whose
decoded length is 1–255 would pass a huge value as a length. Whether a short
cert can survive the `decoded_len == capacity` equality check is exactly the open
question — if capacity is derived from the stored data, it may be reachable; if
it is a fixed 1024, it is not.

### Revised next steps

1. Resolve where the capacity at `[r7+0x14]` is set. This single fact decides
   whether either candidate above is real. The prologue needs manual
   reconstruction around the literal pool.
2. Only then fuzz, and fuzz with correctly-framed payloads:
   `"AZTU" || base64(DER)`.
3. UART remains the multiplier — `ret = %d` from
   *"Verify temp unlock cert fail, ret = %d"* would distinguish every error path.
   Note the return codes are visible statically as `mvn` constants:
   `-8` (`0x1f72`, null ptr/len), `-4` (`0x1f78`), `-5` (`0x1f7e`), `-10`
   (`0x1f84`), `-6` (`0x1f8a`), `-12` (`0x1f90`) — so a single observed `ret`
   value would immediately identify which check failed.

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

## Next steps

1. **Determine the length bound.** Does LK reject >1024 bytes, or does it
   overflow forward into the next IDME entry (`rear_cam_otp`, entry header at
   `0x3158`)? An unbounded copy would be a second, more direct corruption
   primitive. ⚠️ Test carefully — one byte over lands on the `rear_cam_otp`
   entry *name*, which could break IDME parsing. That is recoverable (LK has
   *"IDME initialize failed, force to fastboot mode"*) but would need care, and
   we hold a full `idme_boot1.img` backup.
2. **Fuzz the DER parser** through `flash tucert` — malformed lengths, deep
   nesting for recursion depth, oversized INTEGER/BIT STRING headers, truncated
   sequences. Failure modes are observable: boot messages, `ret = %d` values,
   and whether the device reaches Android at all. Every iteration is a reboot,
   so this wants scripting.
3. **Static-analyse `amzn_verify_temp_unlock_code`** to find the buffer it
   parses into and whether the cert's declared length is trusted. Blocked on the
   Thumb-2 / multi-blob issue in [lk-reversing.md](lk-reversing.md).

## Safety notes

- Restore with `fastboot flash tucert <256 zero bytes>`; verified to work.
- A malformed cert that crashes LK is expected to be recoverable — the field can
  be rewritten from fastboot, and IDME parse failure explicitly forces fastboot
  mode rather than bricking.
- **Do not** attempt this against `flash unlock`; that path *does* verify before
  writing, so it offers no primitive and only burns reboots.

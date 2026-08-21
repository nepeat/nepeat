# The temp-unlock scheme, reconstructed

Recovered from `yacht_lk.bin` 2026-08-21 by disassembling the verifier at
`0x1e00`–`0x2002`. This is the first full account of how Amazon's temporary
unlock actually works on this device. It explains why every write primitive we
have is insufficient.

## Three parts, not one

| part | where it lives | who controls it |
| --- | --- | --- |
| **cert** | IDME `t_unlock_cert`, `"AZTU"` + base64, decodes to 592 B | **we can write it** (unauthenticated) |
| **codes** | IDME `t_unlock_code`, an array of 32-byte entries | fetched at `0xe54c` |
| **signature** | IDME `t_unlock_code`, exactly 256 bytes | fetched at `0x1d3c` |

The decoded 592-byte cert is laid out as:

```
+0x000  payload (336 bytes)   <- signed region
+0x020  embedded key length
+0x024  embedded PUBLIC KEY
+0x150  RSA-2048 signature over the 336-byte payload (256 bytes)
```

## The two phases

**Phase 1 — authenticate the cert against LK's own root key** (`0x1f06`–`0x1f1c`):

```
0x1ef6  bl #0xe58c          ; fetch LK's embedded root pubkey -> (n, e)
0x1f06  stm.w sp, {r2, r3}  ; push it as the key argument
0x1f0a  mov  r0, r4         ; data   = cert
0x1f0c  sub.w r1, sb, #0x100; datalen= 592-256 = 336
0x1f10  add.w r2, r4, #0x150; sig    = cert+0x150
0x1f14  mov.w r3, #0x100    ; siglen = 256
0x1f18  bl #0x19a8          ; RSA verify
0x1f1c  cbnz r0, #0x1f7e    ; fail -> ret -4
```

Then it hashes the cert's key (`0x1f3e`, `bl #0x73f4`) and walks a list fetched
by `0x1c4c`, comparing 32 bytes at a time:

```
0x1f68  bl #0x37bd4         ; memcmp(list[i], hash, 32)
0x1f6c  cbz r0, #0x1f90     ; MATCH -> ret -12
```

A match is a **rejection**, so this is a **revocation / blocklist of cert-key
hashes** — LK can blacklist a leaked temp-unlock key.

**Phase 2 — verify the caller's signature using the CERT's key**
(`0x1fb2`–`0x1fe4`):

```
0x1fbe  ldr r0, [r7, #0x18]   ; codes array
0x1fca  add.w r0, r0, r4, lsl #5   ; code[i], 32 bytes
0x1fc6  movs r1, #0x20        ; datalen = 32
0x1fbc  mov  r2, sl           ; sig    = t_unlock_code (256 B, from IDME)
0x1fce  mov.w r3, #0x100      ; siglen = 256
0x1fc0  add.w r1, r3, #0x24 -> [sp]      ; key    = cert+0x24
0x1fc8  ldr  r5, [r3, #0x20] -> [sp+4]   ; keylen = cert+0x20
0x1fd4  bl #0x19a8
0x1fd8  cbnz r0, #0x1fe6      ; no match -> next code
0x1fe0  ...                   ; "Device is temporarily unlocked"
```

Return value: **0 = unlocked**, `1` = not unlocked (`0x1fea`), negative = error.

So a successful unlock requires a 256-byte signature over one of the 32-byte
codes, made with the private half of the key **embedded in the cert**.

### Where `sl` comes from — RESOLVED, and my earlier guess was wrong

The previous revision of this file listed the signature as "the 256-byte blob
passed to `fastboot flash unlock`", flagged as inferred. **It is not.** The
entry point is a wrapper at `0x1e0c` (the real verifier is `0x1e48`):

```
0x1e0c  push {r0, r1, r4, lr}
0x1e12  mov.w r3, #0x100     ; capacity = 256
0x1e1c  str  r3, [sp]
0x1e1e  bl   #0x1d3c         ; get IDME t_unlock_code (the malloc(0x100) getter)
0x1e26  ldr  r0, [sp, #4]    ; fetched buffer
0x1e2a  ldr  r1, [sp]        ; decoded length
0x1e2c  cmp.w r1, #0x100     ; must be EXACTLY 256
0x1e30  bne  #0x1e42
0x1e32  bl   #0x1e48         ; verifier(buf, 256)  -> sl = buf
```

So `sl` is **IDME `t_unlock_code`**, a second 256-byte IDME field carrying the
signature — the same `"AZTU"`-style container treatment as the cert, with its
own exact-length requirement. Nothing in this path comes from `fastboot flash
unlock` at all.

That makes the whole mechanism **entirely IDME-driven and offline**: a unit is
temp-unlocked by writing two IDME fields, with no live challenge/response.

## Why our write primitive does not help

We can write `t_unlock_cert` freely and unauthenticated — that was verified on
the device. The obvious idea is to install *our own* cert carrying *our own*
public key, then sign a code ourselves.

**Phase 1 blocks exactly that.** The cert is RSA-2048 verified against LK's
embedded root key before its key is ever used, and the cert's key is only read
at `+0x24` *after* `0x1f1c` passes. Substituting a cert means forging an Amazon
signature over the 336-byte payload.

Note the ordering carefully, because it also kills the follow-on idea: the
attacker-controlled key **length** at `[cert+0x20]` is only loaded at `0x1fc8`,
well after verification, so an oversized-key parsing bug in `0x19a8` is not
reachable pre-auth either. Same for the code list and the revocation list — both
are fetched (`0x1c48`, `0x1c4c`) after `0x1f18`.

Everything attacker-controlled is downstream of one RSA-2048 check.

## Status of these claims

**Observed** (with the disassembly above): the two-phase structure, the argument
layout of both `0x19a8` calls, the 336/256 split, the revocation semantics
(match ⇒ reject), the success/failure return values.

**Inferred, not proven**: that `0x19a8` is specifically RSA-PSS rather than
another RSA padding mode; that `0xe58c` returns LK's root key (its position and
use strongly imply it, but the function was not reversed); that the 32-byte
codes from `0xe54c` are device-bound values (e.g. a hash of the DSN/serial) —
their provenance is the next thing to resolve.

The `sl` question is **closed**: it is IDME `t_unlock_code`, not the `flash
unlock` payload. That guess was wrong and is corrected above — which is the
reason for labelling inferences in the first place.

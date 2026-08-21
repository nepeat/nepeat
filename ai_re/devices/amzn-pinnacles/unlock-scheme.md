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

## NEW: `fastboot flash tucode` exists — a second unauthenticated IDME write

The IDME write dispatcher at `0xe4c8` compares the flash-target name against
**two** strings, not one:

```
0xe4cc  strcmp(name, "tucert")   -> 0x1d9c   ; write t_unlock_cert
0xe4fa  strcmp(name, "tucode")   -> 0x1dbc   ; write t_unlock_code
```

with matching failure strings `write tucert failed!` (`0x4bac1`) and
**`write tucode failed!`** (`0x4bad6`).

Every prior note in this project listed the locked-hardware flash surface as
`flash:unlock` and `flash:tucert` only. **`tucode` was never discovered.** It is
the writer for the 256-byte signature field that phase 2 consumes — so both
halves of the temp-unlock credential are writable through the same
unauthenticated path.

**Observed:** the LK-side writer accepts both names, and each has its own
distinct error string.
**NOT yet tested:** whether `flash:tucode` passes fastboot's *locked-hardware
allowlist*. That allowlist is a separate gate, and for `tucert` it was
established empirically, not from this dispatcher. Do not assume `tucode` is
reachable on a locked device until `fastboot flash tucode` has actually been
run against one.

### Why this matters

If `flash:tucode` is permitted on locked hardware, then a complete temp-unlock
credential — cert **and** signature — can be installed on any locked unit with
no root and no Amazon involvement at flash time. The remaining barrier is purely
**obtaining one valid pair**, not installing it.

That reframes the whole problem for the community: the question stops being
"how do we defeat LK's crypto" and becomes "can one valid cert+code pair be
recovered from any unit Amazon ever temp-unlocked, and is it portable?"

## The per-device challenge: `unlock_version`

`0xe27c` reads an **8-byte IDME field `unlock_version`** (`0x4ba09`). If it is
absent or its first word is zero, LK **generates a value and writes it back**:

```
0xe29a  movw r6, #0xc181
0xe29e  movs r0, #0
0xe2a0  bl   #0x44248        ; rand()
0xe2a4  muls r0, r6, r0      ; * 0xc181
0xe2a6  adds.w r4, r0, #0x11 ; + 0x11
0xe2aa  beq  #0xe29e         ; retry if 0
0xe2ac  cmp  r4, #0x30
0xe2ae  beq  #0xe29e         ; retry if 0x30
0xe2ba  bl   #0x208          ; idme_write("unlock_version", &r4, 8)
```

So the device mints its own per-unit value on first use and persists it. It is
then formatted with `0x%08x%08x%08x` (`0x4b9fa`) — a 96-bit rendering — into the
BSS buffer at `0x9d244`, alongside a 45-byte IDME field read into `0x9d234`, and
the strings `unlock_code` (`0x77115`) and `unlock_status` (`0x77128`) label this
area.

**Inferred, not proven:** that the 32-byte codes consumed by phase 2 are derived
from `unlock_version` (and therefore that a cert+code pair is bound to one unit).
The codes table at `0x9d264` is **BSS**, populated at runtime, so it is *not*
baked into the image — my first hypothesis that the codes were static constants
shared across all units was checked and is **wrong**.

**This is now the single most important open question**, because it decides
whether a recovered credential is portable:

* if the codes derive from `unlock_version`, each unit needs its own Amazon
  signature — but note LK *generates* that value itself, so if `unlock_version`
  can be written (it is an IDME field), a unit could potentially be made to
  match a credential we already have;
* if the codes are constant across units, one leaked pair unlocks the family.

Resolving where `0x9d264` is filled from is the next step.

### Where the codes table is filled — partially traced

`0xe534` is the setter: `memcpy(0x9d264, src, 0x144)`, and **`0x144` = 324 =
4-byte count + 10 × 32-byte entries**, matching the getter's `count <= 10` bound
exactly. That confirms the table layout.

Its single caller is `0x11188`, inside a **tag-dispatched TLV parser**:

```
0x11184  add.w r0, r4, #8
0x11188  bl    #0xe534        ; codes := record payload
```

The parser is `0x10d40` (entered from `0x11270`, no arguments — it reads a
global). Records carry a 16-bit tag at `+2` and payload at `+8`, and sibling
cases write into a large config struct at base `+0x5880` / `+0x5900` / `+0x5980`
— the same struct the codes getter gates on (`ldrb [r2, #0x59a6]` at `0xe55e`).
The struct is `0x5d70` bytes (`0x10d78`).

**So the 32-byte codes arrive as one TLV record inside a larger config blob**,
not from IDME and not as image constants.

**Not yet identified: where that blob comes from.** Candidates are the
preloader→LK handoff, a config partition, or a runtime-built structure.
`0x2bef6`, called just before the parse loop, turns out to be a bare `bx lr`
stub, so it is not the loader. Literal resolution inside `0x10d40` lands in a
data island and needs care — the naive `ldr`/`add pc` formula returns garbage
there, so the globals at `0x10d46`/`0x10d50` were **not** resolved.

This is the remaining thread on portability. If the blob is preloader-supplied,
the codes are outside reach without preloader code execution. If it comes from a
writable partition, that is a much more interesting story — but nothing here
establishes which, and it should not be guessed.

## Why `tucert`/`tucode` bypass the locked-hardware gate — the call path

This explains the long-standing empirical puzzle: why `fastboot flash tucert`
succeeds on a locked device while every other flash target is refused.

The fastboot `flash` handler is `0x33538`, taking `(name, data, size)`. Its
order of evaluation:

```
0x33548  bl #0x37c0e          ; strcmp(name, "unlock")   [str @0x4b689]
0x3354c  cbnz r0, #0x33558    ; not "unlock" -> fall through
0x33552  bl #0xe468           ;   "unlock"   -> dedicated verified handler
0x3355e  bl #0xe4c8           ; IDME dispatcher: "tucert" / "tucode"
0x33562  cmp r0, #0
0x33564  bne.w #0x33692       ; HANDLED -> return immediately
0x3356e  bl #0x37c0e          ; strcmp(name, "partition") [str @0x7ac92]
0x33578  ...                  ;   -> "Attempt to write partition image."
0x3357e  ...                  ;      "Do not support this operation." [0x7a75c]
0x33584  bl #0x4028c          ; partition lookup by name
0x335f0  bl #0x44ca8          ; ... the locked-hardware checks live down here
0x335f8  cmp r0, #1
```

`0xe4c8` returns **1 when it handled the name** (`0xe4f6`) and **0 otherwise**
(`0x33562`'s `bne` therefore exits the function). So a `tucert` or `tucode`
flash is fully serviced and **returns before the lock gate at `~0x335f0` is
ever evaluated**.

**This is a call-path argument, not an inference from strings.** It predicts
exactly the behaviour already observed on the device for `tucert`, and it puts
`tucode` on the *same* pre-gate path — same function, same early return, same
dispatcher.

**Still not run against hardware.** The prediction is strong and mechanism-based,
but `fastboot flash tucode` has not been executed on a locked unit, and it will
not be recorded as confirmed until it has.

### Net effect

Both halves of the temp-unlock credential — the cert and the 256-byte signature
— are installable on a locked, unrooted device through a path that never
consults the lock state. The bootloader's entire defence rests on the RSA-2048
signature inside the cert, and nothing else.

### Refinement: the allowlist is the real gate, and `tucode` is on it

The call-path argument above (early return before the checks at `~0x335f0`) is
accurate but is **not the primary mechanism**. The actual gate is an explicit
command allowlist consulted by `0xdb14`, verified by dereferencing the pointer
table at `0x8366c` (relocation base `0x56000000`):

```
ALLOWED when restricted:  oem relock | oem flags | flash:unlock
                          flash:tucert | flash:tucode
RESTRICTED (@0x83680):    verify | dump | boot | env | signature | oem
                          | flash | erase
```

So **`flash:tucode` is explicitly permitted on locked hardware** — this is no
longer a prediction from control flow, it is a named entry in the allowlist
table. The "untested" caveat now applies only to running it on the device, not
to whether it is reachable.

See [fos-flags.md](fos-flags.md) for the surrounding analysis.

## ⛔ SUPERSEDED: the credential is a per-boot nonce, not something recoverable

The framing above — "installing is easy, so the question becomes whether a valid
pair can be recovered and is portable" — is **answered, and the answer is no.**

The ten 32-byte codes are `HMAC-SHA256(S_device, counter)`, where `S_device` is
32 bytes minted by the unit's own RNG and sealed in **eMMC RPMB block 1**, and
`counter` is incremented and rewritten to RPMB **every boot**. A cert+code pair
therefore cannot unlock a different device, and expires within 10 reboots on the
device it was issued for.

Amazon's temp unlock is an **online challenge/response**, not a portable
credential. `flash:tucert` and `flash:tucode` remain genuine unauthenticated
writes — there is simply nothing durable to write.

Full evidence, independently verified, in
[unlock-codes-rpmb.md](unlock-codes-rpmb.md).

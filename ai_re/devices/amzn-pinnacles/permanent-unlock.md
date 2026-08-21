# The PERMANENT unlock path — and the one lead that could still break this open

Distinct from the temp-unlock scheme in [unlock-scheme.md](unlock-scheme.md).
Mapped 2026-08-21 from `yacht_lk.bin` (base 0).

## The mechanism

`amzn_is_unlocked_permanent()` at `0x196c`:

```
0x1970  memset(sp, 0, 0x100)
0x1984  -> 0x77115 "unlock_code"
0x1986  bl #0x274        ; idme_get_var_external("unlock_code", sp, 0x100)
0x198a  cbnz r0 -> return 0
0x1992  bl #0x1af4       ; amzn_verify_unlock(buf, 256)
0x1996  return (ret == 0)
```

`amzn_verify_unlock` (`0x1af4`, named by its own log strings at `0x48016`/`0x48054`)
assembles a verify over three inputs:

| operand | source | detail |
| --- | --- | --- |
| **key** | `0xe3a8` → `0x4b8d4` | hard-coded RSA-2048 DER SPKI, 294 B, sha256 `de6344d4…` |
| **message** | `0xe380` → `0xe250` | 26 ASCII bytes, `"0x%08x%08x%08x"` |
| **signature** | IDME `unlock_code` | 256 B |

verified by `0x19a8` = `rsa_verify_hash_ex(padding=3 → **LTC_PKCS_1_PSS**,
sha256, saltlen 32)`, success being an exact `stat == 1` test at `0x1aa0`.

**The key is not the temp-unlock root key.** Permanent uses `0x4b8d4`; temp uses
`0x4baec` (`0xe58c`). LK carries six RSA-2048 SPKIs in total: `0x473df`,
`0x47939`, `0x4b8d4`, `0x4baec`, `0x57384`, `0x574ac`. None appears verbatim in
the preloader.

### The signed message is a static per-device string

```
0xe250  format(buf, v):
  0xe254  movs r0,#0xd ; bl #0x11db0    -> devinfo[13]
  0xe25e  movs r0,#0xc ; bl #0x11db0    -> devinfo[12]
  0xe26e  r1 -> 0x4b9fa "0x%08x%08x%08x"
  0xe270  bl #0x37886                   ; snprintf(buf, 0x29, fmt, devinfo[13], devinfo[12], v)
```

i.e. **`"0x" + %08x(devinfo[13]) + %08x(devinfo[12]) + %08x(unlock_version)`**,
26 bytes. This is exactly the string published by `fastboot getvar unlock_code`
(`0xe3f0`).

`unlock_version` is IDME, 8 bytes; when absent/`0`/`0x30`, LK **mints it
itself**: `rand()*0xc181 + 0x11` (`0xe27c`), then writes it back.

**Crucially: no counter, no nonce, no expiry, no revocation list.** Unlike the
temp path (RPMB per-boot HMAC, ≤10 reboots), a permanent unlock issued for a
unit is **valid forever and survives OTA**.

## `flash:unlock` → `0xe468`, complete

```
0xe468  cmp r1, #0xff ; bhi 0xe476
0xe470    -> "signature length error, wrong signature file? do nothing!"
0xe476  mov.w r1, #0x100        ; length FORCED to 256; input size discarded
0xe47a  bl #0x1af4
0xe47e  cbz r0 -> 0xe48c
0xe480    -> "unlock signature verify failed, do nothing!"
0xe48c  -> "unlock_code" ; r2 = 0x100
0xe496  bl #0x208               ; idme_update_var("unlock_code", data, 256)
0xe4a2  bl #0xe3bc              ; refresh unlock_status
0xe4ae  b.w #0x312f8            ; fastboot_okay
```

**Verify-then-write.** No write before verification, no early return leaving
state set. The `>= 256` gate instead of `== 256` is sloppy but **inert** — `0x1af4`
is invoked with a hardcoded `0x100`, and LTC rejects `siglen != modulus length`
regardless; surplus bytes are never read.

Audited for the usual weaknesses, all negative: no non-constant-time or
non-cryptographic compare; no pre-validation use of attacker data beyond the PSS
operand; and **no stack overflow from the 1024-byte field into the 256-byte
buffer** — the V2 getter `0xa24` clamps (`0xa62: cmp r2,sb ; it hs ; movhs r2,sb`).

The permanent path has **strictly less pre-auth attack surface than the temp
path**, which base64-decodes an attacker-supplied `AZTU` container before any
signature check.

## No lock flag exists — the credential *is* the state

`amzn_is_restricted()` **recomputes** from IDME on every call (two full RSA-2048
verifies per invocation). Nothing is cached, so **relock and unlock cannot be
desynchronised** — a clean negative on that idea.

### `oem relock` (`0xe2e0`) — and a reversibility quirk worth knowing

```
0xe2f6  strncmp(arg, " keep-version", 13)
0xe304  bl #0x1ddc      ; clear t_unlock_cert and t_unlock_code
0xe30e  (no keep-version) rand()*0xc181+0x11
0xe32c  bl #0x208       ; idme_write("unlock_version", &new, 8)   <-- REROLL
0xe348  bl #0x208       ; idme_write("unlock_code", buf, 0)       <-- clears permanent credential
```

* **`fastboot oem relock keep-version` is reversible.** It preserves
  `unlock_version`, so the signed message is unchanged and a previously issued
  256-byte blob can simply be re-flashed with `fastboot flash unlock` later.
* **A bare `oem relock` is destructive.** It rerolls `unlock_version`, which
  permanently invalidates any Amazon-issued unlock for that unit.
* `oem relock` is on the locked-hardware allowlist, so anyone with fastboot
  access to an unlocked unit can burn the owner's unlock with one command. An
  availability bug, not a bypass.

## Not reachable from the unauthenticated writers

`0xe4c8` compares only `"tucert"` and `"tucode"`, dispatching to `0x1d9c` /
`0x1dbc`, each of which **hardcodes its own IDME name**. Neither can reach
`unlock_code` or `unlock_version`. `oem flags` can only write `usr_flags` when
restricted.

## ⭐ THE OPEN LEAD: are `devinfo[12]`/`devinfo[13]` per-chip unique?

The signed message has exactly three inputs. `unlock_version` is a 32-bit value
the **device mints itself** into a writable IDME field — and we have root, which
can write `mmcblk0boot1` directly.

So **if `devinfo[12]` and `devinfo[13]` are family constants rather than
per-chip unique**, then one Amazon-issued permanent unlock blob from *any*
`yacht`/`pinnacles` unit would unlock *every* unit: set that unit's
`unlock_version` to match the blob, write the 256-byte signature into IDME
`unlock_code`, done. Permanent, OTA-surviving, no per-device Amazon involvement.

**What their source is — resolved here:** the preloader's devinfo
source-register table (`0x383a0`, 8-byte `(register, count)` entries) maps

```
entry 12 @0x38400  reg = 0x11f10140   -> devinfo[12]
entry 13 @0x38408  reg = 0x11f10144   -> devinfo[13]
```

(The table validates itself: entry 27 → `0x11f10060` and entry 28 → `0x08000000`
chipid, both matching values already read from this device.)

So they are **two consecutive eFuse words at `+0x140`/`+0x144`** — which is the
classic location for a per-chip unique ID on MediaTek parts. That makes
"per-chip unique" the strong prior, and the lead correspondingly weak.

**But it is not proven, and the test is free.** Both addresses lie inside the
eFuse page `0x11f10000`–`0x11f10fff`, which is on the USBDL **read** whitelist —
the same path already used to read `0x11f10060 = 0x946`. So:

```
READ32 0x11f10140      # devinfo[12]
READ32 0x11f10144      # devinfo[13]
```

is **read-only, requires no root, and carries zero risk**. Equivalently,
`fastboot getvar unlock_code` is registered **ungated** (`0xe3f0`, published
before any restriction check) and returns the whole 26-byte string directly.

**The decisive comparison needs a second unit.** Read `unlock_code` (or those two
eFuse words) from this device and from any other `yacht`/`pinnacles`-class unit
and compare the first 16 hex digits. Identical ⇒ family constants ⇒ the whole
family is unlockable from one blob. Different ⇒ per-chip unique ⇒ dead.

⚠️ Those 16 hex digits are a **device identifier** — treat them like the DSN and
keep them out of public posts.

## Key-sharing structure across the family — measured

Compared all RSA-2048 SPKIs in `yacht_lk.bin` against the two reference `trona`
LK images in `ref-firmware/`. Six keys each, at structurally corresponding
offsets. **Four of six are shared; the two that differ are exactly the unlock
keys.**

| slot (yacht) | sha256 prefix | role | shared with trona |
| --- | --- | --- | --- |
| `0x473df` | `2af78db6…` | image verification | **yes** |
| `0x47939` | `3ab387e5…` | image verification | **yes** |
| `0x4b8d4` | `de6344d4…` | **permanent unlock** | **NO — yacht-only** |
| `0x4baec` | `851766d2…` | **temp unlock root** | **NO — yacht-only** |
| `0x57384` | `85d7f6f7…` | image verification | **yes** |
| `0x574ac` | `be68a42f…` | image verification | **yes** |

The two `trona` images (`PS7326`, `PS7331`) share **6/6** keys with each other,
so keys are **stable per variant across firmware versions**.

This refines the community claim that "Amazon uses different signing keys per
device". More precisely: **the image-signing keys are common across the Amazon
MT8183 family, and only the unlock keys are per-variant.**

Two consequences:

1. **A `trona` unlock blob can never work on `yacht`** — different unlock key,
   independent of anything else. Cross-variant credential reuse is dead.
2. **Within `yacht`, every unit's LK carries the same permanent-unlock key**
   (`de6344d4…`), since keys are baked into LK and stable across versions. So a
   blob issued for one `yacht` unit verifies on another **iff the signed message
   matches** — i.e. iff `devinfo[12]`, `devinfo[13]` and `unlock_version` match.

`unlock_version` is settable (IDME, and root can write `mmcblk0boot1`
directly). **So the entire question reduces, cleanly, to whether `devinfo[12]`
and `devinfo[13]` are unit-unique** — exactly the free, read-only test described
above. Nothing else stands in the way.

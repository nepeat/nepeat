# `fos_flags` / `dev_flags` — dm-verity and SELinux switches, and why they are a decoy

LK really does contain switches to turn dm-verity off and force SELinux
permissive. They are **not** a route to running modified code on a locked
device, and the reason is now proven rather than assumed.

## The switches are real, and the bits are known

One primitive does both tests — `fos_flags_test(class, _, mask_lo, mask_hi)` at
`0x2bf28`:

```
0x2bf28  cmp  r0, #2
0x2bf34  ldr r7,[pc,#0x58]/add r7,pc   -> 0x4b6c2 'dev_flags'   (class != 2)
0x2bf3a  ldr r7,[pc,#0x58]/add r7,pc   -> 0x4b6d6 'fos_flags'   (class == 2)
0x2bf4a  [sp+4]='0' ; [sp+5]='x'
0x2bf5e  bl #0x274                      ; idme_get_var_external(name, sp+6, 8)
0x2bf64  bl #0x37340                    ; strtoul("0xNNNNNNNN")
0x2bf70  ands r0,r4 / ands r1,r5
0x2bf74  cmp/cmpeq                      ; require (val & mask) == mask
0x2bf7c  bl #0xdaf2                     ; <-- THE GATE
```

| switch | site | flag | bit |
| --- | --- | --- | --- |
| dm-verity **off** | `0x2bfb0` → `0x2bfee` | `fos_flags` | **`0x80`** |
| SELinux **permissive** | `0x2c02c` → `0x2c04a` | `dev_flags` | **`0x40`** |
| SELinux force-enforcing | `0x2c02c` → `0x2c030` | `dev_flags` | `0x20` (checked first, wins) |

Cross-validated: `0x2f7f6` calls the same primitive with `fos_flags & 0x4` to set
`printk.disable_uart=0`, matching `FOS_FLAGS_CONSOLE_ON=0x4` in the recovery
ramdisk's `init.fosflags.sh`. LK and Android share the bit namespace; `0x80` is
LK-only and absent from the shell script.

## Storage: plain IDME fields, ASCII hex

From LK's baked-in IDME descriptor table (32-byte entries
`{u32 size; u32 flags; u32 perm; u32 default_ptr; char name[16]}`, validated
against the known 1024-byte `t_unlock_cert`):

| field | file offset | size | perm |
| --- | --- | --- | --- |
| `dev_flags` | `0x832c8` | `0x20` | `0444` |
| `fos_flags` | `0x832e8` | `8` | `0444` |
| `usr_flags` | `0x83308` | `8` | `0444` |
| `t_unlock_code` | `0x83388` | `0x200` | |
| `t_unlock_cert` | `0x833a8` | `0x400` | |

Physically the IDME block in **eMMC boot1** (`mmcblk0boot1`); values previously
located at `0x2290` (dev), `0x22b4` (fos), `0x22d8` (usr). **Not** boot_para,
misc, nvcfg, or the TLV struct. A full string-xref sweep finds these names at
exactly 13 sites: the `oem flags` handler (`0xdda6`–`0xe048`) and `0x2bf28`.

## Two independent walls

**Wall 1 — you cannot write them.** `fastboot oem flags` *is* permitted on
locked hardware (it is on the allowlist, below), and `print` is completely
ungated. But the setter gate is:

```
0xdfd0  bl #0xdc70
0xdfd4  cbz r0 -> 0xdff8            ; unrestricted: any class
0xdfd6    class == 3 (usr) -> write
0xdfe4    class == 0 (unspecified) -> usr_flags
0xdfea    else -> 'Only usr_flags can be set for a locked device' -> FAIL
```

Only `usr_flags` is writable when restricted — and **`usr_flags` is read by
nothing**: the test primitive can only select `dev_flags` or `fos_flags`
(`0x2bf28: cmp r0,#2`), and Android's `init.fosflags.sh` merely `export`s
`USRFLAGS` without acting on it.

Parser detail (`0xdd8e`–`0xde46`): class prefixes `f:`/`fos:`/`fos_flags:` → 1,
`d:`/`dev:`/`dev_flags:` → 2, `u:`/`usr:`/`usr_flags:` → 3; modifiers `=` assign,
`+` OR (`0xe0c4`), `-` BIC (`0xe0ba`); write via
`idme_update_var_ex(name, "%llx", 0x12)` at `0xe0de`.

**Wall 2 — and this one makes Wall 1 moot.** The gate `bl #0xdaf2` sits *inside*
`fos_flags_test` itself. So even with root, even using the unauthenticated
`tucert` IDME write, even by editing `mmcblk0boot1+0x22b4` directly, LK reads the
value, matches the mask, and then **returns 0 anyway** on a restricted device.
All four callers of `0x2bf28` are the dm-verity and SELinux sites. **There is no
ungated consumer.**

These flags are a *post*-unlock developer convenience, not a pre-unlock bypass.

> **Consequence for a planned experiment:** `lk-reversing.md:199` proposed
> writing `dev_flags=1` to `mmcblk0boot1+0x2290` and checking `getenforce`. That
> is now **predicted to do nothing**, for a proven reason rather than an unknown
> one. (The bit was wrong too — permissive is `0x40`, not `1`.)

## Corrections to earlier notes

**1. `0xdaf2`/`0xdc70` return 1 = RESTRICTED.** They are byte-identical
duplicates of `amzn_is_restricted()`, not "is unlocked":

```
bl #0xe234 ; cmp r0,#1 ; bne -> return 0    ; boot_arg->byte[0x59a7] != 0
bl #0x196c ; cmp r0,#0 ; bne -> return 0    ; IDME 'unlock_code' must NOT verify
bl #0x1e0c ; return (r0 == 0)               ; temp-unlock must NOT verify
```

**2. Byte `+0x59a7` is NOT a lock byte — it is `androidboot.prod`.** `0x302de`
reads it straight into `'androidboot.prod=%d'` (`0x78c21`); its neighbour
`+0x59a6` feeds `androidboot.rpmb_state=%d`, matching the live `rpmb_state=2`.
This retires the "the lock state is one byte, and LK does not own it" framing in
`lk-emulation.md`.

**3. "LK never writes `+0x59a7`" is false.** `0x10efe strb.w r1,[r6,#0x27]`
(r6 = struct+`0x5980`) writes it, inside the TLV parser at `0x10d40`, under TLV
tag **`0x886100a3`** (payload `[8..0xb]` → `+0x59a4..+0x59a7`). The blob is the
**preloader hand-off in DRAM**, with a `'LPLP'` magic fast-path at `0x10d6e`
that `memcpy`s the whole `0x5d70` struct verbatim. Not attacker-reachable
without preloader control, but the categorical wording must go.

## The locked-hardware allowlist — verified directly

Command tables, relocated at base `0x56000000` (verified by reading the pointer
table and dereferencing, independently of the analysis above):

```
ALLOWED when restricted  @0x8366c[5]:
   oem relock | oem flags | flash:unlock | flash:tucert | flash:tucode
RESTRICTED               @0x83680[8]:
   verify | dump | boot | env | signature | oem | flash | erase
```

`0xdb14` short-circuits with `if (!amzn_is_restricted()) return 0;`.

**`flash:tucode` is explicitly on the allowlist.** That independently confirms
the prediction in [unlock-scheme.md](unlock-scheme.md): both halves of the
temp-unlock credential are writable on a locked, unrooted device.

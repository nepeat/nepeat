# Static analysis breakthrough — and where the unlock decision actually lives

The emulation idea paid off, though not the way expected: **full emulation turned
out to be unnecessary.** Getting proper Thumb-2 tooling in place (via the
unicorn/capstone work) cracked the static analysis, and that answered the
architecture question outright.

## ⚠️ Read this first: a bricking risk we did not previously understand

`t_unlock_cert` is parsed **on every boot**, on a **locked** bootloader, by code
that runs **before** signature verification. If a blob makes LK crash during that
parse, the result is a **permanent boot loop with no way back** — you cannot
`fastboot flash tucert` a replacement if LK dies before the fastboot menu is
reachable.

**Do not write a crashing blob until recovery is proven.** Specifically, confirm
on serial where the tucert parse sits relative to fastboot entry, and that the
hardware key combo reaches fastboot *before* the parse. This upgrades UART from
"nice for visibility" to **a prerequisite for touching this path at all.**

## The xref problem: solved

Every earlier attempt failed because linear disassembly desyncs through the
interleaved data. The fix is to **scan from every 2-byte boundary**, decode a
short window, and accept a `ldr rX,[pc,#imm]` … `add rX,pc` pair only when it
resolves to a known string. False positives are essentially impossible.

All twelve target strings resolved — and note the whole image is base 0,
including the `0x76xxx` cluster, so the **earlier "multi-blob with differing
bases" theory was wrong**; the scanner was simply broken:

| string | referenced from |
| --- | --- |
| `amzn_verify_unlock` | `0x1b1a`, `0x1b38` |
| `amzn_verify_temp_unlock_code` | `0x1eaa`, `0x1fa6` |
| `Failed to get temp unlock cert` | `0x1ea8` |
| `Verify temp unlock cert fail` | `0x1fa4` |
| `Device is temporarily unlocked` | `0x1fde` |
| `t_unlock_cert` reads | `0x1d14`, `0x1daa`, `0x1de8` |
| `t_unlock_code` reads | `0x1d74`, `0x1dca`, `0x1df4` |
| **`Only usr_flags can be set for a locked device`** | **`0xdff0`** |
| `0x%08x%08x%08x` | `0xe26e` |
| `[DM-VERITY] verify off by fos_flags` | `0x2bfee` |
| `[SELINUX] set to permissive mode by dev_flags` | `0x2c05e` |

(Ghidra's original xrefs — `0x1b1a` for `amzn_verify_unlock` — were **right after
all**. Earlier notes dismissed them as ARM-decode artifacts. The addresses were
correct; only the instruction decoding was wrong.)

## The lock state is one byte, and LK does not own it

`0xdfd0` in the `oem flags` handler calls `0xdc70`, which is a thin wrapper:

```
0xdc70  push {r3, lr}
0xdc72  bl   #0xe234        ; the real check
0xdc76  cmp  r0, #1
0xdc78  beq  #0xdc7e        ; unlocked path
0xdc7a  movs r0, #0         ; locked
```

and `0xe234` — `amzn_target_is_unlocked` — is tiny:

```
0xe234  ldr  r3, [pc, #0x14]
0xe23a  add  r3, pc
0xe23c  ldr  r3, [r3]
0xe23e  ldr  r2, [r3]        ; double indirection -> global struct
0xe236  movw r0, #0x59a7
0xe240  ldrb r1, [r2, r0]    ; ONE BYTE at struct+0x59a7
0xe244  it   ne
0xe246  movne r0, #1
0xe248  bx   lr
```

**The entire runtime lock state is a single byte** in a global structure.

Crucially, all four instructions in the image that reference offset `0x59a7`
(`0xe236`, `0x302de`, `0x31328`, `0x319d8`) are **reads** — three of them just
log it. Since `0x59a7` exceeds Thumb's 4095-byte immediate range, a write would
also need a `movw`, and there is none. **LK never sets this byte.** It arrives in
the boot-args structure. Neighbouring fields at `0x59a6`, `0x59a8`, `0x59aa`,
`0x59ac`, `0x59ae` are read the same way, consistent with `g_boot_arg`.

## The unlock verification is in the PRELOADER

That redirects the whole investigation. Strings in `preloader_boot0.img`:

```
AMZN_UNLOCK
AMZN_PL_VERIFY
[%s] Can't read IDME (error code=%d)
[%s] Fail to read unlock version
[%s] Fail to read unlock signature
[%s] Fail to compose unlock code
[%s] SEC CFG is valid. Lock state is %d
[%s][%s] Error: fail to do rsa2048 public key decryption.
[%s][%s] Only try verify %s with prod key on locked production device
[%s][%s] Succeed to verify %s with eng key.
%s Tool connection is unlocked
%s usbdl_verify_da: da_len (0x%x) is less than sig_len (0x%x)
```

So the **preloader** reads IDME, composes the unlock code, reads the signature
and version, does the RSA-2048 verification, and hands the resulting lock state
to LK. LK is downstream of the decision, which is exactly why it only ever reads
that byte.

**The preloader is therefore the correct target for a boot-time unlock**, and it
is a smaller binary than LK. Extracted and ready:

- Container: `EMMC_BOOT` header → `BRLYT` at `0x200` → GFH at `0x800`
- `GFH_FILE_INFO`: **load address `0x00200D00`**, length `0x3c4e8` (247,016 B),
  max size `0x80000`, content offset `0x300`
- Payload extracted from file offset `0xb00` → `scratchpad/yacht_preloader.bin`
- `AMZN_UNLOCK` log tag referenced from `0x276a`–`0x2840`; that region is the
  IDME block-read helper (sector arithmetic, malloc, read, memcpy)

Note also `usbdl_verify_da` — the preloader's USB download path verifies the
Download Agent, which is the surface any mtkclient/DA-style attack would need.

## LibTomCrypt: version pinned, bugs found, no code execution

Independent analysis pinned the vendored library to **1.18.2** from the build
path (`der_decode_subject_public_key_info.c` exists only in the 1.18 line; it was
renamed after 1.18.2). Findings, measured against a real 1.18.2 build:

**Unbounded recursion in `der_decode_sequence_flexi` — real and unfixed.** The
1.18.2 "fix" ([PR #373](https://github.com/libtom/libtomcrypt/pull/373)) checks
depth *after* the recursive call returns, walking the `->child` chain. The actual
fix ([`cac400c`](https://github.com/libtom/libtomcrypt/commit/cac400cf79), 2020,
[issue #533](https://github.com/libtom/libtomcrypt/issues/533)) **has never
shipped in a release** — 1.18.2 is still the newest tag. Measured: **298 nesting
levels in 1021 bytes**, needing **~12–17 KB of stack** against an LK stack that
is typically 8 KB. Indefinite-length `30 80` does *not* work as a 2-byte cheat
(`_fetch_length` rejects it), so definite lengths cap it at 298.

**32-bit-only integer overflow in `_fetch_length` — apparently novel.**
`return z + *data_offset` wraps on a 32-bit `unsigned long`, so `30 84 FF FF FF
FF` yields a child parser handed `inlen = 0xFFFFFFFF`, walking off the end of the
1024-byte buffer. On 64-bit it is correctly rejected, which is why it survived —
and libtomcrypt has no oss-fuzz coverage at all.

**But there is no write primitive.** Every sub-decoder linked into our image
bounds its writes with an `*outlen` check returning `CRYPT_BUFFER_OVERFLOW`
(`der_decode_octet_string`, `bit_string`, `object_identifier`, `utf8_string`).
900,000 guard-page fuzzing iterations against real 1.18.2 produced exactly one
memory-safety fault — CVE-2019-17362, a **one-byte** OOB read via `0c 01 81`,
with no output channel and therefore useless here.

**Honest verdict:** reliable crash yes, unbounded OOB read yes, **controlled
memory corruption or code execution — no, not from any known public bug.** The
only corruption is flexi's own stack frames (saved LRs and heap pointers, not
attacker data). Turning that into control flow would need a bespoke LK
memory-map exploit that does not exist publicly.

Reproducers generated (in scratchpad, not committed): `nest298.bin` (1021-byte,
298-deep), `crash1.bin` (`0c 01 81`).

## The preloader's unlock decision — mapped end to end

Applied the same xref technique to `yacht_preloader.bin`. `unlock_code` is
referenced at `0x28f2`, `unlock_version` at `0x2920`, and the whole routine sits
at roughly `0x28dc`–`0x2a10`.

**It reads IDME at hardcoded byte offsets**, and they match our parsed IDME
table exactly:

```
0x028fc  movw r2, #0x4ec     ; = IDME entry offset of unlock_code
0x0290a  mov.w r3, #0x100    ; 256 bytes = RSA-2048 signature
...
0x0292e  movw r2, #0x2afc    ; = IDME entry offset of unlock_version
0x02932  movs r3, #4         ; read as 4 bytes -> confirms the 32-bit %08x
0x02948  movw r2, #0x26fc    ; a second read, 4 bytes (backup/secondary copy)
```

Then it composes the signed message in place — `strb '0'`, `strb 'x'` at
`0x2980`, building `0x%08x%08x%08x`:

```
0x029c4  strb r0, [r4, #0x1a]   ; NUL-terminate at 26 chars = "0x" + 24 hex digits
0x029c6  bl   #0x203c           ; fetch the verifying key
0x029cc  movs r1, #0x1a         ; message length 26
0x029de  bl   #0x2048           ; <<< RSA VERIFY
0x029e6  clz  r0, r0
0x029ea  lsrs r0, r0, #5        ; branchless (result == 0) ? 1 : 0
0x029ec  str  r0, [r3]          ; <<< STORE LOCK STATE
0x029ee  cbnz r0, #0x29f6
0x029f0  ...'locked'  /  0x029f6 ...'unlocked'
```

That confirms the message is **exactly 26 bytes**, `"0x"` plus three 32-bit
values as `%08x` — matching the format string found in LK.

**There is no logic flaw in this check.** `clz(r0) >> 5` is a branchless exact
equality test against zero — no sloppy comparison, no signed/unsigned confusion,
no early-out. The result goes straight into the lock-state global that is later
handed to LK as the byte at boot-arg `+0x59a7`.

There is one cached-state guard at the top:

```
0x028e6  ldr  r5, [r3]
0x028e8  cmn.w r5, #0xff        ; is the cached state == -255 (uncomputed)?
0x028ec  bne.w #0x2a0c          ; already computed -> return cached value
```

so the verification runs once per boot and is memoised. Nothing exploitable
there either — the sentinel is in preloader RAM, not attacker-reachable storage.

**Verdict: the unlock verification chain is clean.** Preloader reads IDME →
composes a 26-byte device-bound message → single RSA-2048 verify → exact
zero-test → lock state. No flaw found at any step. This closes the "find a bug
in the unlock check" line of attack.

## Where this leaves the goal

The tucert DER path gives a **denial of service, not an unlock** — and per the
warning above, firing it blind risks an unrecoverable brick. It should not be
touched before UART.

**The preloader is now the more promising target**, because it holds the actual
unlock decision, it is extracted with a known load address, and it has not been
examined at all yet. Next steps there:

1. Locate the RSA verification and the "compose unlock code" routine.
2. Determine exactly which bytes are hashed — we already know the format string
   is `0x%08x%08x%08x` over SoC_ID, HW_ID, `unlock_version`.
3. Audit its length/parse handling the way LK's was audited.
4. Check `usbdl_verify_da` for the DA-verification weaknesses that mtkclient-class
   attacks rely on.

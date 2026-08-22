# LK parses the boot header BEFORE verifying it — and yacht is missing trona's size guard

> **⛔ RESOLVED: INERT. The missing guard is real and the wrap is reachable, but
> it is not load-bearing** — two independent downstream checks that yacht *does*
> have already cover everything it would have caught. Verified by emulation. See
> the verdict at the end. Kept in full because the parse-before-verify ordering
> is a genuine and reusable finding.

**The most promising surface found on this project.** Two independent facts that
compose, both verified in our own binary.

## Fact 1 — the header is parsed before signature verification

Caller at `0x44560`, the `r4 == 0` (boot) path:

```
0x445a2  mov r0, r5 ; mov r1, r7
0x445a6  bl #0x125ec        ; PARSE boot header
0x445aa  subs r4, r0, #0
0x445ac  bge #0x445b8       ; parse succeeded ->
0x445ae  ...                ; parse failed -> error, bail
0x445b8  mov r0, r5 ; mov r1, r6
0x445bc  bl #0x12ce4        ; VERIFY image   <-- ONLY AFTER THE PARSE
0x445c0  subs r4, r0, #0
0x445c2  bge #0x445ce       ; verify succeeded ->
0x445c4  ...                ; "Failed to verify %s" -> bail
0x445ce  mov r0, r6
0x445d0  bl #0x3fae8        ; boot
```

`0x125ec` is the parse function (returns at `0x12924`); `0x12ce4` is the verify
function, which owns the strings `Header hash verification` (`0x12e82`),
`amzn_image_verify: boot image` (`0x12f46`), `Failed to verify %s` (`0x12fcc`).
They are **separate functions** — address adjacency proves nothing, which is why
this needed the caller traced. Each has exactly one caller
(`0x125ec` ← `0x445a6`, `0x12ce4` ← `0x445bc`).

**So every field in the boot image header is consumed while the image is still
entirely unauthenticated.**

## Fact 2 — yacht lacks the size validation trona has

`strings -c` over three builds:

| image | `image size error` | `image page size error` |
| --- | --- | --- |
| **yacht** (ours, build `20220815`) | **0** | **0** |
| trona PS7326 (`20231205`) | 2 | 2 |
| trona PS7331 | 2 | 2 |

The arithmetic is the same shape in both, reading the standard Android header:

```
0x12848  ldr r5, [r4, #0x24]     ; page_size
0x12860  ldr r3, [r4, #8]        ; kernel_size
0x1286c  ldr r0, [r4, #0x10]     ; ramdisk_size
0x12878  ldr r1, [r4, #0x28]     ; header_version ; if == 1:
0x12880  ldr lr, [r4, #0x660]    ;   recovery_dtbo_size
0x128aa  mla r3, r5, fp, r2      ; page_size * kernel_pages + base — 32-bit, UNBOUNDED
0x128b0  str r3, [r4]            ; stored to a global
```

Trona adds a bound immediately after its `mul` (`0x136f4: ldr r0,[r7,#0x24] ;
cmp r4,r0`) with a dedicated error string. **Yacht stores the product and
proceeds.** Our build predates the fix by ~16 months.

## Why this is reachable without an unlock

We have **root** (CVE-2022-38181), so `boot` (`mmcblk0p16`) and `recovery`
(`mmcblk0p17`) are writable with `dd`. A malformed header does not need to pass
verification to be *parsed* — parsing is what happens first. The signature check
is what stops it *booting*, not what stops it being read.

## What is NOT yet established — do not overstate this

* **Whether the parse can actually be driven into memory corruption.** `0x125ec`
  returns a status that the caller checks with `bge`, so *some* validation
  exists; it simply lacks trona's explicit size/page-size bound. What `0x125ec`
  rejects on its own has not been enumerated.
* **What the `mla` product is used for.** It is stored to globals at `0x128b0`
  and `0x128ec`. Whether those feed a copy destination, a length, or only a
  bookkeeping value decides whether an overflow is exploitable or inert.
* **Whether an overflowed value reaches a write.** Not traced.

Until those three are answered this is a **strong lead, not a vulnerability.**
Recorded that way deliberately — this project has retracted ~10 plausible
findings today, and "the guard is missing" is not the same as "the guard was
load-bearing".

## Related surface, same open question

`amzn_lk_verify_image_maybe` (`0x2c860`) handles exactly four image names —
`cam_vpu1`, `cam_vpu2`, `cam_vpu3`, `spm` (`mmcblk0p13/14/15/11`), each with a
trailing `_VPUx_VER:` / `__SPM_VER:` tag and a 256-byte signature. Unknown names
are rejected at `0x2c8c4`, so the "maybe" is benign. But these are
**root-writable partitions parsed by LK** and were not previously on anyone's
list. They deserve the same parse-vs-verify ordering audit.

## Why this is original

There is **no published bootloader unlock for any Amazon MT8183 device** and
**no public analysis of Amazon's LK image-verification path**. Every historical
Fire unlock went through the BootROM, which is fused off here. If this pans out,
it is new work.


---

# ⛔ VERDICT: inert missing check — closed

Reversed the full chain and emulated the parser. **The guard was genuinely
missing and genuinely not load-bearing.**

## What `0x125ec` actually validates

Only two things: the `ANDROID!` magic (`0x12818`) and **`page_size <= 0x800`**
(`0x1283e: cmp.w r0,#0x800 ; bhi.w 0x1271c`). Not validated: `page_size != 0`,
power-of-two, `kernel_size`, `ramdisk_size`, `header_version`,
`recovery_dtbo_size`, or any product bound. `0x44e24` is `__aeabi_uidiv` and
**explicitly handles a zero divisor**, so `page_size = 0` is benign rather than a
fault.

## Where the products actually go

Resolved through the GOT (base `0x93148`, link base `0x56000000`):

| global | runtime | meaning |
| --- | --- | --- |
| `0x4ac` | `0x5609d9ac` | `g_loadbase` = **`0x56900000`** (hardcoded, `0x13a4c`) |
| `0x3a4` | `0x5609d9b8` | `g_rootfs_off` = the **`mla`** product |
| `0x1c`/`0x3b4` | `0x5609d9a0`/`0x5609d9b0` | `g_total` = the **`mul`** product |

`g_total` **is** used as both a copy length and a copy destination
**pre-authentication**, in three flash reads at `0x12e26`/`0x12e46`/`0x12e66`,
all before the header hash (`0x12ed6`) and the RSA verify (`0x12f2c`). So the
concern was well-founded in shape.

`g_rootfs_off` is dereferenced as a raw pointer at `0x12ff2` — but only after the
hash passes **and** (`sig_ok == 1` or `is_unlocked()`). Locked with a bad
signature never reaches it. Dead for us.

## Why it is still inert — two guards yacht does have

**(a) `0x12208` `range_check(addr, size)`**, called at `0x12d16` with
`(loadbase, g_total)` *before* any read:

```
0x12208  adds r3, r0, r1 ; bhs -> -1        ; 32-bit add-overflow rejected
0x12210  cmp.w r1, #0x1400000 ; bhi -> -1   ; size capped at 20 MiB
0x12230  addr < 0x56000000 -> require addr+size < 0x56000000
0x12244  else                -> require addr >= 0x56400000
```

**(b) `0x41adc` (partition read) bounds in 64 bits**, so the wrapped values
cannot slip through:

```
0x41b0c  bl 0x403b8              ; partition size, 64-bit r0:r1
0x41b10  adds r2,r6,r5 ; adc r3,r7,#0   ; offset+len in 64 bits — cannot wrap
0x41b1a  cmp/cmpeq ; bhs -> reject       ; require partsize >= offset+len
```

The underflowed `total - 2*page_size` (~4 GiB) is rejected by (b); the
destination is confined by (a).

**Residual effect, real but useless:** a wrapped `g_total` puts read #1 up to
`0x1000` below the load base — a ≤2048-byte write of attacker-controlled header
bytes into `0x568FF000–0x56900000`, a region `range_check` already blesses (LK
legitimately decrements `loadbase` by `0x200` at `0x12dc0`). Read #2 or #3 then
fails and verify returns `-5`. No control of anything.

## Emulation, with an exact oracle

Unicorn over `0x12838`→`0x128f0` with the real `__aeabi_uidiv`. Against the
**real `boot.img`** (`ps=0x800, kernel=0x98a830, rd=0, hv=1`):

```
g_total = 0x0098c000   ->  read2 offset = 0x98b800
boot.img[0x98b800] = 30 82 04 35 ...     <- the Amazon DER signature cert
last non-zero byte  = 0x98bc38            <- inside that same page
```

The model predicts the signature's exact location from the header arithmetic —
independently re-verified here. Mutations:

```
ps=0,      k=0x1000,     rd=0x1000  -> total=0          (no fault; uidiv handles /0)
ps=0x800,  k=0xFFFFF800, rd=0xFFFFF800 -> total=0       read1 dst=0x568FF000 len=0x800
                                                        read2/3 rejected by 0x41adc
ps=0x800,  k=0xFFFFF800, rd=0, hv=0   -> total=0x800    read3 len=0xFFFFF800 rejected
ps=0x800,  k=0xFFFFFFFF               -> total=0x1000   benign (add wraps first)
ps=0x7FF,  k=rd=0xFFFFFFFF            -> total=0xFFE    benign
```

## What Amazon actually added in trona

Two instructions, both absent from yacht:

```
0x12a10  cbnz r1, #0x12a2a      ; page_size != 0   -> "boot image page size error"
0x12ae2  cmp  r4, r2
0x12ae4  bhs  #0x12afa          ; total >= page_size -> "boot image size error"
```

i.e. precisely the 32-bit multiply-wrap guard plus a zero check. Yacht's
recovery-side twins (`0x129e4` parse, `0x13120` verify, `range_check` at
`0x13152`) are missing both in the same way — and are inert for the same reasons.

## Conclusion

**"The guard is missing" was not the same as "the guard was load-bearing."**
Recording it as a lead rather than a vulnerability was the right call. No
`boot.img` needs crafting and nothing should be flashed.

**The parse-before-verify ordering stands as a real finding** and is worth
carrying forward: any *future* bug in the header path is reachable
pre-authentication, and the three flash reads at
`0x12e26`/`0x12e46`/`0x12e66` all execute on unauthenticated data.

**Minor leads, not chased:** the `0x670` header buffer is filled from flash with
no NUL guarantee and `hdr+0x40` (cmdline) is passed to a `strlen`/`memcmp`
keyword scanner at `0x3fb48` — an unterminated cmdline gives a heap **over-read**,
read-only, at worst a fault.

---

# `amzn_lk_verify_image_maybe` (the `cam_vpu*`/`spm` surface) — closed

Flagged earlier as an unaudited surface: `cam_vpu1/2/3` (`mmcblk0p13/14/15`) and
`spm` (`mmcblk0p11`) are **root-writable partitions** with an LK verifier, and
nobody had checked the parse-vs-verify ordering on them.

**Result: the LK-side verifier is well-formed AND appears to be dead code.**

## It rejects unknown images correctly

The function (body at `~0x2c860`) strcmp-chains four names and picks a version
tag for each:

```
0x2c876 "cam_vpu1" ─┐
0x2c884 "cam_vpu2"  ├─> "_VPUx_VER:"   (0x2c8ac / 0x2c8b2 / 0x2c8b8)
0x2c890 "cam_vpu3" ─┘
0x2c89c "spm"       ──> "__SPM_VER:"   (0x2c8a6)
0x2c8a2 cbnz r0, #0x2c8c4              ; no match ->
```

The unknown-name path was the concern — a name-allowlist verifier that no-ops on
unknown input is exactly "missing proper image authentication". It does not:

```
0x2c8c4  -> "AMZN_LK_VERIFY" + "...doesn't support verifying %s"
0x2c8cc  b #0x2ca84
0x2ca84  mov r2,r4 ; mov r3,r4
0x2ca88  b #0x2c8f0
0x2c8f0  bl #0x374b2         ; log it
0x2c8f4  mov.w r5, #-1       ; <-- REJECT
0x2ca9a  mov r0, r5 ; pop    ; return -1
```

**Unknown images return `-1`.** Clean negative.

## And nothing calls it

- **No `bl` callers** to any address in `0x2c850`–`0x2c874` (the neighbourhood
  was swept, having learned from the BROM off-by-4 that a single wrong entry
  address manufactures a fake "no callers" mystery).
- **No pointer-table entry.** The only 4-byte match anywhere is `0x0002c86a` at
  file `0x1dd60` — a *file-relative* value pointing mid-function, whereas real LK
  pointers are `0x56000000`-based. Coincidental data.
- The `cam_vpu1/2/3` strings are referenced from **exactly one place each**
  (`0x2c876`, `0x2c884`, `0x2c890`) — inside the verifier itself. Nothing else in
  LK mentions them.

So LK carries the verifier but never invokes it. The live verification of these
images is presumably the **preloader's** `amzn_pl_verify_image_maybe` — a
different function, with its own name allowlist and its own
`"doesn't support verifying %s"` string, which is where the question properly
belongs.

**Verdict: no LK-side surface here.** Whether the *preloader* copy rejects
unknown names the same way is the open question, and is the one that matters,
since the preloader is what actually loads these images.

*(Aside: there is a descriptor table at file `0x88ca5`–`0x88e54` holding `spm`
and `cam_vpu1/2/3` names at a 0x20 stride — plainly an image/partition table,
reached as data rather than via `ldr`/`add pc`. Not chased.)*

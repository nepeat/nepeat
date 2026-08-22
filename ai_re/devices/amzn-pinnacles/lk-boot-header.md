# ⭐ LK parses the boot header BEFORE verifying it — and yacht is missing trona's size guard

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

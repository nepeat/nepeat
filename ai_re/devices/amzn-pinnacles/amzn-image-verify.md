# Amazon's image-verification layer — audited, correctly implemented

Motivated by **CVE-2022-20060** ("missing proper image authentication",
preloader/usb, MT8183). Amazon adds its own verifier on top of MediaTek's, with
a **name allowlist** — a shape that would match that CVE exactly if it no-opped
on unknown names. **It does not.** Audited both copies; both are sound.

## Timing context

| component | build stamp |
| --- | --- |
| preloader | **`20220401-090835`** (from `%s PL Build Time: %s`) |
| LK | `pinnacles-e6b8902-20220815071944` |

ALPS06160806 (CVE-2022-20055/56/58/59/60) is in MediaTek's **March 2022**
bulletin, so our preloader postdates publication by ~3 weeks. Whether it carries
the fix is still open — OEMs get patches ahead of publication — but the Amazon
layer below is not where that bug could live.

## The preloader verifier — `amzn_pl_verify_image_maybe`

Entry **file `0x215c` / VA `0x00202e5c`**. Allowlist, by `strcmp` (`0x25378`):

```
0x216c  strcmp(name,"lk")    -> 0x21a0 -> tag "_LK_VER:"
0x2178  strcmp(name,"sspm")  -> 0x21a6 -> tag "_SSPM_VER:"
0x2184  strcmp(name,"vpu")   -> 0x21ac -> tag "_VPUx_VER:"
0x2190  strcmp(name,"spm")   -> 0x21b2 -> tag "__SPM_VER:"
0x2196  (no match) -> "AMZN_PL_VERIFY" / "doesn't support verifying %s"
0x219e  b #0x232e
```

**Unknown names are rejected**, not silently accepted:

```
0x232e  bl #0x22bcc          ; log
0x2336  b  #0x234a
0x234a  mov.w r7, #-1        ; <-- REJECT
0x234e  mov r0, r7 ; pop     ; return -1
```

An empty version tag also falls into the same error path (`0x21c0 beq #0x2196`).

## It is called, and the result is enforced

Four wrappers call it — `0x247a` (`lk`), `0x24ae` (`sspm`), `0x24e8` (`vpu`),
`0x251a` (`spm`) — each a **tail call** (`bl` immediately followed by
`pop {…, pc}`), so the verdict propagates rather than being swallowed.

And at the top of the security-critical path:

```
0x19312  bl  #0x2458        ; verify "lk"
0x19316  cmp r0, #0
0x19318  bne #0x1938c       ; verification failed -> bail out
0x1931a  ...                ; only on success: proceed to load LK
```

**So the preloader authenticates LK, and enforces the result.** The second
caller of that wrapper (`0x1ffd2`) merely stores the value with
`strh r0,[r5,#0x12]` — a status-reporting path, not the boot decision.

## The LK-side copy is dead code

`amzn_lk_verify_image_maybe` (body `~0x2c860`) has a *different, smaller*
allowlist — `cam_vpu1`, `cam_vpu2`, `cam_vpu3`, `spm` — and also correctly
returns `-1` for unknown names (`0x2c8f4`). But it has **no callers and no
pointer-table entry**, and the `cam_vpu*` strings are referenced from exactly one
site each, all inside the function itself. LK carries the code and never runs it;
the live verification happens one stage earlier, in the preloader. See
[lk-boot-header.md](lk-boot-header.md).

Note the preloader's allowlist is the broader one (`lk`, `sspm`, `vpu`, `spm`) —
it covers **LK itself**, which LK obviously cannot do for itself.

## Verdict

**Clean negative.** Amazon's image-authentication layer is correctly implemented
in both copies: unknown images are rejected, the result is propagated through
tail calls, and the LK boot path branches away on failure. CVE-2022-20060, if it
applies to this build at all, is **not** in the Amazon verifier — it would have
to be in MediaTek's stock code, most plausibly the DA/USBDL path.

## Method note

Both audits initially produced a false "no callers anywhere" result from an entry
address that was slightly wrong — the same trap that manufactured the BROM's
phantom mystery (stub entries recorded 4 bytes too low). The fix is to **sweep a
window of candidate entry addresses** rather than trusting one, and to check for
pointer-table references before concluding a function is unreachable. Both
sweeps are what turned "no callers" into `0x215c ← {0x247a, 0x24ae, 0x24e8,
0x251a}`.

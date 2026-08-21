# USBDL PMIC commands bypass the memory whitelist — the one surviving primitive

All addresses verified directly against `yacht_preloader.bin` (load base
`0x00200D00`; VA = file offset + `0x200D00`).

## The whitelist tables, read out directly

This settles the address-filter question exactly, and reveals a region the
15-address probe never tested:

```
g_write_whitelist @file 0x3ad8c:
   [0] 0x10007000  size 0x1000     (WDT)
   [1] 0x1001a080  size 0x4        <-- never probed
g_read_whitelist  @file 0x3ad9c:
   [0] 0x10007000  size 0x1000     (WDT)
   [1] 0x1001a080  size 0x4        <-- never probed
   [2] 0x11f10000  size 0x1000     (eFuse page)
```

Two consequences worth recording:

* **The eFuse page is readable but NOT writable.** `0x11f10000` appears only in
  the read list. That is why `READ32 0x11f10060` returned `0x946` while writes
  elsewhere returned `0x1001`, and it retires the earlier worry about
  accidentally programming fuses through this path — the write filter would
  refuse.
* **`0x1001a080` (4 bytes) is both readable and writable** and was never probed.
  It is a single word in the MT8183 peripheral space. Worth a read-only probe.

The empirical probe and this table agree completely, which cross-validates both.

## Primitive: `usbdl_pwr_write16` (cmd `0xC7`) — no validation at all

`VA 0x205dcc` / file `0x50cc`, verified:

```
0x205dcc  add r0, sp, #0x2c ; bl #0x4858      ; get_word(&addr)
0x205dda  mov r0, r4        ; bl #0x4858      ; get_word(&data)
0x205dec  movs r0, #0       ; bl #0x47fc      ; put_word(0)  <-- HARDCODED status
0x205df2  ldrh.w r0,[sp,#0x2c]                ; addr
0x205df6  mov r1, r5                          ; data
0x205df8  movw r2, #0xffff                    ; mask = full
0x205dfc  movs r3, #0                         ; shift = 0
0x205dfe  bl #0x216f0                         ; pmic_config_interface(...)
```

Two 16-bit words go straight into `pmic_config_interface` with a full `0xffff`
mask. **`sec_region_check` is never called on this path** — the PMIC commands do
not go through the memory whitelist at all, which is exactly why the earlier
address probe never saw them. The parameter-check status is a hardcoded `0`,
matching upstream MediaTek's own comment: *"check whether address and data are
valid, dummy in this implementation."*

**Why it survives our fuse state.** It is orthogonal to all three enforcement
mechanisms. SBC and DAA gate *what code is authenticated*; this is a data-path
command inside the already-running, already-authenticated preloader. BROM being
fused off is irrelevant — this lives in the preloader USBDL handler we have a
working handshake with. No credential, no signed image, no partition write, no
flash access.

**What it offers.** Software control of PMIC rails while the preloader runs —
i.e. voltage glitching with no hardware rig. The natural target is the RSA check
inside `usbdl_verify_da`: brown out the core rail during verification, then send
an unsigned DA.

### Staged test plan — the first two steps are safe

1. **Prove the path is live, read-only.** Issue `CMD_PWR_READ16` (`0xC6`)
   against a benign MT6358 register (chip-ID/status). Success looks like two
   echoed words plus status `0`, instead of the `0x1000`/`0x1001` region-check
   failures. That alone demonstrates the PMIC path bypasses the whitelist.
2. **Prove write reachability without changing a rail.** `CMD_PWR_WRITE16`
   (`0xC7`) writing a register's *current* value back to itself, then read back.
   Status `0`, value unchanged. No electrical change.
3. Only then consider actual rail manipulation.

### ⚠️ Step 3 is not free, and should not be described as such

Writing PMIC registers can set an out-of-spec core voltage, and some MT6358
registers latch power-sequencing behaviour. A brown-out can land in a state that
does not recover — and with BROM fused off and **no BROM-producing test point on
this board family**, there is no unbrick path. Treat any rail write as
potentially terminal.

Glitch tuning also normally needs UART or a current-shunt side channel to know
*when* execution is inside the verify routine. This board's UART pads are
firmware-disabled, so blind tuning would be a long grind with a low prior.

**Confidence:** that the primitive exists as described — **high, verified on our
binary**. That it can be turned into a working unlock — **low**. This is a
research direction, not a shortcut.

## Two related primitives, both refuted on our silicon

Recorded so they are not re-attempted. Both came from public MediaTek preloader
source (`svoboda18/preloader`, byte-identical across mt6580/6765/6768/6779/6785)
and **do not hold against Amazon's hardened build**.

**`is_in_region` integer overflow — REFUTED.** Upstream computes
`start + size` unchecked, so a wrapping length passes the upper-bound test. On
`yacht`, `sec_region_check` (`VA 0x225ba0`) opens with an explicit carry check
*before* anything else:

```
0x225ba0  cmn r0, r1        ; flags for addr + len
0x225ba6  blo 0x225bbe      ; normal path ONLY if no carry-out
                            ; carry set -> error 0x00105002
```

and `is_in_region` (`VA 0x225b04`) was hardened independently — upstream has two
comparisons, this has five, including an explicit end-vs-start self-overflow
test at `0x225b22`. The earlier conclusion that the filter bounds-checks
`addr + count*4` is now confirmed at instruction level.

**`CMD_SEND_DA` missing `da_region_check` — real but useless.** There genuinely
is no region check (`0x205924  movs r0, #0` — a hardcoded status, not a computed
one; `da_region_check` does not exist as a symbol). **But the destination is
pinned:**

```
0x205946  ldr r0, [pc, #0x2b0]   ; CFG_DA_RAM_ADDR = 0x40200000 (verified @file 0x4ef8)
0x20594e  str r0, [sp, #0x24]    ; da_addr := 0x40200000 -- attacker value DISCARDED
0x205954  blx r3                 ; read(dst=0x40200000, len=da_len)
```

The `da_addr` you send is echoed back and then overwritten. `da_len` remains
unclamped, so it is a linear overflow running *upward* from a fixed
`0x40200000` — but preloader code, stack and structures live in SRAM around
`0x00200000`–`0x0023xxxx`, i.e. *below* the origin. Reaching them requires
wrapping the address space: ~3.06 GB of USB bulk transfer across all of DRAM and
the entire MMIO aperture. It will fault or wedge on protected MMIO long before
wrapping. Not worth device time.

## Note on the koboreru reading

This preloader is demonstrably a **hardened** build relative to every public MTK
BSP in the one place examined closely (`sec_region_check` / `is_in_region`). That
makes "the `check_part_overlapped` string is absent because the routine was
removed or rewritten" more plausible than a neutral reading of a missing string
would suggest — so the evidence against koboreru applying should be weighted a
little more strongly.

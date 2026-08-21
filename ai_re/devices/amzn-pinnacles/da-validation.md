# DA validation is conditional — the best root-free lead

**Found in the preloader: `usbdl_verify_da` skips Download Agent signature
validation entirely on a "non-secure chip".** If MediaTek Secure Boot Control
(SBC) is not fused on this unit, preloader USBDL will accept an **unsigned DA** —
which means full flash read/write with **no root, no BROM exploit, and no
bootloader unlock**.

Whether that applies here comes down to **one bit**, and the preloader prints it
at every boot.

## The bypass, verbatim

`usbdl_verify_da` (preloader, `~0x4cc0`–`0x4e40`):

```
0x04d16  bl   #0x2cbb8        ; secure-chip query
0x04d1c  cmp  r0, #1
0x04d1e  beq  #0x4d34         ; SECURE -> fall through to real verification
0x04d20  ldr  r0, [pc, …]     ; "DA validation disabled on non-secure chip"
0x04d22  movs r5, #0          ; r5 = 0
0x04d2e  bl   #0x22bcc        ; print
0x04d32  b    #0x4e10         ; -> mov r0, r5  ==> returns 0 = VALIDATED
```

`r5 = 0` is the success value — the same value the real verification path returns
via `DA authenticated`. So on a non-secure chip the function **reports the DA as
valid without checking anything**.

The secure path (taken only when the query returns exactly 1) does the real work:
init key, bound `da_len` against `DA_RAM_LENGTH` (`0x120000`), require
`da_len > sig_len`, then `bl #0x24b60` to verify, with failure printing
`DA validation fail` and dumping `LK DA signature` / `LK DA pubk` (a `0x100`-byte,
i.e. RSA-2048, key).

## The deciding bit

`0x2cbb8` is four instructions:

```
0x2cbb8  ldr  r3, [pc, #8]
0x2cbba  ldr  r0, [r3]
0x2cbbc  ubfx r0, r0, #2, #1     ; extract bit 2
0x2cbc0  bx   lr
```

The literal at `0x2cbc4` is **`0x11f10060`**.

`0x11f10000` is the MT8183 **eFuse controller** — independently confirmed on the
live device, which exposes `/sys/devices/platform/11f10000.efusec` and a
devicetree node `efusec@11f10000`. So the register is `efuse_base + 0x60`, and:

| bit | meaning |
| --- | --- |
| **2** | **SBC enabled** — gates DA signature validation (this finding) |
| **8** (`0x100`) | `EFUSE_Disable_BROM_CMD` — the BROM lockout (community-reported) |

**That is a nice corroboration:** the community's `efuse_base+0x60` bit `0x100`
claim for the BROM fuse — previously second-hand from XDA — is confirmed by our
own preloader reading the very same register at the very same offset. One
register answers both open questions.

## Why this matters more than anything else found

Every other avenue needed something we do not have:

| path | blocker |
| --- | --- |
| forge an unlock cert | Amazon's RSA-2048 private key |
| tucert DER parser | crash only, no write primitive, and bricking risk |
| BROM / mtkclient | possibly fused off; needs physical BROM entry |
| force-BROM-recovery | destroys the preloader; unrecoverable if fused |
| Mali root | per-boot, and an OTA would patch it |

**This one needs none of them.** If bit 2 is clear:

1. Enter **preloader USBDL** (`0e8d:2000`) — which, importantly, is a *different*
   mode from BROM (`0e8d:0003`) and is reported to remain reachable even where
   BROM is fused. R0rt1z2's own guidance on these devices is *"find an exploit in
   the preloader, which is still accessible."*
2. Send an arbitrary **unsigned DA**.
3. The preloader prints `DA validation disabled on non-secure chip` and accepts
   it.
4. The DA has full eMMC access → dump and flash anything, including a custom ROM.

No root, no unlock, and **OTA-proof**, since it lives in the preloader rather
than in a kernel bug.

## Is bit 2 actually clear here? Unknown — and cheap to answer

Arguments it may be **set** (DA validation enforced): `ro.boot.secure_cpu=1`,
`verifiedbootstate=green`, and the preloader plainly does verify LK.

Arguments it may be **clear**: Amazon runs their *own* image verification layer
(`AMZN_PL_VERIFY`, `amzn_image_verify`, *"Only try verify %s with prod key on
locked production device"*) which is **independent of MediaTek SBC**. A vendor
that authenticates images themselves does not necessarily also blow the MTK SBC
fuse. This is genuinely undetermined.

**How to find out, in order of cost:**

1. **UART — free, zero risk, no modification.** The preloader already prints
   `sbc_enabled: 0x%x` and `daa_enabled: 0x%x` at every boot, plus
   `[EFUSE] sbc: %x` and `[EFUSE] sbc_key_hash is correct/incorrect`. **The device
   tells you the answer out loud on every single boot.** This is now clearly the
   highest-value hardware task on the project.
2. **Root + kernel read of `0x11f10060`.** We have an arbitrary-read primitive
   from the Mali chain. There is no `/dev/mem`, and the `efusec` platform device
   exposes no readable attributes (`driver_override`, `modalias`, `uevent` are
   all permission-denied, and there is no `nvmem` interface), so this needs the
   exploit's read primitive rather than a file read.
3. **Empirically** — enter preloader USBDL and offer a deliberately unsigned DA.
   If it is accepted, bit 2 is clear. Requires physical button entry, but is
   read-only in effect: a rejected DA changes nothing.

## Next steps

- **Get UART.** It answers this, the BROM fuse question, and the tucert-fuzzing
  visibility problem all at once.
- Determine the physical button combo for preloader USBDL entry (`0e8d:2000`) as
  distinct from BROM (`0e8d:0003`).
- If bit 2 turns out clear, the whole goal reduces to writing or adapting a DA —
  and mtkclient already ships DAs for MT8183 (hwcode `0x788`).
- Note `Tool connection is unlocked` (`~0x18fca`) is still unexamined and may be
  a related state worth understanding.

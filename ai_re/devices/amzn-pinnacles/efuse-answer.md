# The eFuse question — ANSWERED

**`efuse 0x11f10060 = 0x00000946` on this device.**

```
0x946 = 0b1001_0100_0110
  bit 1 = 1   sbc_enabled            -> Secure Boot Control
  bit 2 = 1   daa_enabled            -> DA validation is ENFORCED
  bit 8 = 1   EFUSE_Disable_BROM_CMD -> BROM download is FUSED OFF
```

> **Correction (2026-08-21):** earlier revisions of these notes labelled **bit 2
> as SBC**. It is not — **bit 2 is `daa_enabled`** (Download Agent
> Authentication) and **bit 1 is `sbc_enabled`**. The preloader has three
> near-identical fuse stubs 16 bytes apart, all reading `0x11f10060`:
>
> | stub | extract | stored to | meaning |
> | --- | --- | --- | --- |
> | `0x2cba8` | `ubfx r0,r0,#1,#1` | sec block **+8** | `sbc_enabled` |
> | `0x2cbb8` | `ubfx r0,r0,#2,#1` | sec block **+9** | `daa_enabled` |
> | `0x2cbc8` | `and r0,r0,#1` | — | (bit 0) |
>
> The `+8`/`+9` mapping is fixed by the caller at `0x1ff0a`–`0x1ff1a`, which
> calls the two stubs back to back and `strb`s the results to `[r4,#8]` and
> `[r4,#9]`. That matches the security-block layout already confirmed against
> `ro.boot.rpmb_state` / `ro.boot.prod`.
>
> **Both bits are set, so no conclusion changes** — DA validation is enforced
> either way. But the label matters: `usbdl_verify_da` gates on the **DAA** stub,
> not the SBC one, which is exactly why the observed failure is
> `DAA_SIG_VERIFY_FAILED`.

Both open questions are closed, and neither answer is the one we wanted. No UART
and no kernel module were needed — the value was already in data pulled from the
device.

## How it was obtained

The preloader builds MediaTek's `devinfo[]` array by reading a **table of eFuse
register addresses**, then passes the array to the kernel as a devicetree atag.
So the fuse values are readable from `/proc/device-tree/chosen/atag,devinfo`
without any hardware access.

The table lives at `0x383a0` in `yacht_preloader.bin`, with 8-byte
`(register, count)` entries:

```
0x383a0: 0x11f10020 0x00000001     <- index 0
0x383a8: 0x11f10030 0x00000001
   ...
0x38478: 0x11f10060 0x00000001     <- index 27  *** the SBC register ***
0x38480: 0x08000000 0x00000001     <- index 28  (chipid)
```

Index = `(0x38478 - 0x383a0) / 8` = **27**. Every entry before it has `count=1`,
so the mapping to `devinfo[27]` is direct. (Entries from `0x38528` onward have
`count=3` and would shift later indices, but that is past our target.)

Reading `atag,devinfo` (804 B = a `0xC8`=200 count word plus 200 u32s):

```
devinfo[27] @0x070 = 0x00000946     <- efuse 0x11f10060
devinfo[28] @0x074 = 0x00000788     <- chipid
```

**The sanity check is what makes this trustworthy:** index 28 maps to the chipid
register `0x08000000`, and it reads back **`0x788` — the MT8183 hwcode**, exactly
matching `hwparam.json` from the reference project. The index mapping is
verified, not assumed.

## Consequences

**1. The unsigned-DA bypass does not apply.** `usbdl_verify_da` skips signature
validation only on a non-secure chip; bit 2 is set, so this chip is secure and DA
signatures are checked against the RSA-2048 `LK DA pubk`. The most promising
root-free lead of the project is **closed**. See
[da-validation.md](da-validation.md).

**2. BROM download really is fused off**, on *this* unit specifically — no longer
a second-hand XDA claim. `mtkclient` and every BROM-based technique are out.

**3. ⚠️ The "force BROM download recovery" path is now a CONFIRMED BRICK, not a
risk.** [brom-recovery.md](brom-recovery.md) describes a preloader routine that
zeroes its own `preloader` partition to drop into USB download mode. With bit 8
set there is **no BROM command handler to fall back to**, so triggering it leaves
the device with no valid preloader and no recovery path: **permanently,
unrecoverably bricked.** Do not trigger it under any circumstances.

That also resolves the tension noted there. Amazon shipped a recovery path that
cannot work on this hardware — evidently inherited common source retained across
products where the fuse policy differs, not evidence that BROM is open.

## What survives

Everything at the bootloader level is now closed: unlock forging, LK and
preloader verification logic, the fastboot allowlist, DA loading, BROM, and the
DER parser. See [theories-closed.md](theories-closed.md).

The remaining route to a modified system is **root plus kernel code execution** —
an unsigned kernel module (`CONFIG_MODULE_SIG` is not set) used to neuter
dm-verity at runtime, with `masp_hal_set_dm_verity_error` as an exported entry
point worth examining. That is a *soft* mod: it does not unlock the bootloader
and does not survive without re-rooting each boot, but it is the only live path
to running modified system code.

The upside for that route: there is **no OTA client on this image**
([network-behavior.md](network-behavior.md)), so the usual risk of an update
patching the exploit does not apply here.

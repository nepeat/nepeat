
## ⛔ LEAD CLOSED: `devinfo[12]`/`[13]` are per-chip unique

Resolved 2026-08-21 **read-only from the booted device**, no bootloader
interaction: `/proc/device-tree/chosen/atag,devinfo` (804 B, count word 200) —
the same source that previously yielded `devinfo[27]`/`[28]`.

Both sanity oracles matched exactly, validating the index mapping:

```
devinfo[27] = 0x00000946   (efuse 0x11f10060)  ✔ matches the live fuse read
devinfo[28] = 0x00000788   (chipid)            ✔ matches MT8183 hwcode
```

And the answer:

```
devinfo[11] = 0x00000460          <- structured
devinfo[12] = <high entropy>      <- efuse 0x11f10140   ) four consecutive
devinfo[13] = <high entropy>      <- efuse 0x11f10144   ) random words =
devinfo[14] = <high entropy>                            ) a 128-bit
devinfo[15] = <high entropy>                            ) per-chip ID
devinfo[16] = 0x00000000          <- structured
```

Indices 12–15 are four consecutive **high-entropy** words bracketed by
structured neighbours — the signature of a **128-bit per-chip unique ID**
(MediaTek HRID) at eFuse `0x11f10140`–`0x1114c`. They are **not** family
constants.

*(Raw values deliberately not recorded here — they are a device identifier of the
same sensitivity as the DSN. sha256 of the 16-byte block is kept in the commit
trail for future comparison against another unit.)*

**Therefore the permanent-unlock lead is dead.** The signed message contains
`devinfo[13]` and `devinfo[12]`, which differ per unit, so a blob issued for one
`yacht` unit cannot verify on another — regardless of the shared per-variant key
and regardless of being able to set `unlock_version`.

Both unlock paths are now closed for credential reuse:

| path | why reuse fails |
| --- | --- |
| temp | codes are `HMAC(RPMB device secret, per-boot counter)` — per-unit **and** self-expiring |
| permanent | message contains a 128-bit per-chip eFuse ID — per-unit |

Amazon bound each path to a different per-device root: RPMB for temp, eFuse for
permanent. There is no leaked-credential shortcut for this family.

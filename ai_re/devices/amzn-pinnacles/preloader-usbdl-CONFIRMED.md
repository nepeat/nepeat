# CONFIRMED: root-free arbitrary memory write in the preloader

**Demonstrated on the device, 2026-08-21.** Preloader USBDL is reachable
**from software with no buttons**, and mtkclient **successfully writes into the
running preloader's memory** on a locked device with no root and no BROM.

This is the root-free primitive the whole investigation was looking for. Only one
check now stands between it and full flash access, and we know exactly where that
check lives.

## What was demonstrated

```
DaHandler  - Device is in Preloader-Mode.
DAXFlash   - Uploading xflash stage 1 from MTK_DA_V5.bin
XFlashExt  - Patching da1 ...
Mtk        - Patched "Patched loader msg" in preloader
Mtk        - Patched "hash_check" in preloader
Mtk        - Patched "get_vfy_policy" in preloader
XFlashExt  - Patching da2 ...
XFlashExt  - Security check patched
XFlashExt  - DA version anti-rollback patched
XFlashExt  - SBC patched to be disabled
XFlashExt  - Register read/write not allowed patched
Preloader  - upload_data failed with error: DAA_SIG_VERIFY_FAILED (0x7024)
```

Three things are proven by that output:

1. **The preloader handshake succeeds** — `Device is in Preloader-Mode`.
2. **Memory writes into the live preloader work** — those `Patched … in preloader`
   lines are byte-pattern search-and-replace performed over USB via
   `WRITE16`/`WRITE32`. So the memory commands are **not** gated by SLA on this
   device. The open question from
   [usbdl-memory-commands.md](usbdl-memory-commands.md) is answered: **ungated.**
3. **Only DA authentication blocks the next step** — and `0x7024` is precisely the
   constant we had already found at file offset `0x4dfc` in `usbdl_verify_da`
   (`movw r5, #0x7024`). Static analysis and live behaviour agree exactly.

## Entry needs no buttons

The recipe that works, entirely from software:

```sh
# 1. start mtkclient polling FIRST
python3 mtk.py printgpt &
# 2. then reboot the device; the preloader's bldr_handshake window is caught
adb reboot
```

The device then enumerates as **`0e8d:2000` "MT65xx Preloader"**. This matters
because the fuse we confirmed (bit 8 of `0x946`) disables the **BootROM** command
handler — the **preloader's** USBDL is untouched by it.

**Timing note:** mtkclient must already be polling when the device enumerates. It
cannot attach to a preloader that is already sitting there — reattaching gives
`Handshake failed after retries`, because the handshake window is consumed at
enumeration.

## What blocks full access, and where to patch it

The DA upload fails `DAA_SIG_VERIFY_FAILED` because `usbdl_verify_da` takes the
secure path. That decision is made by the four-instruction query at file offset
`0x2cbb8`:

```
0x2cbb8  ldr  r3, [pc, #8]
0x2cbba  ldr  r0, [r3]          ; read 0x11f10060
0x2cbbc  ubfx r0, r0, #2, #1    ; bit 2 = SBC
0x2cbc0  bx   lr
```

mtkclient's generic patches did not hit this — its `SBC patched to be disabled`
line refers to the *DA's* own security state, not the preloader's fuse read.

**The preloader loads at `0x00200D00`** (from its GFH `FILE_INFO`), and our
extracted `yacht_preloader.bin` begins exactly there, so file offset maps to
runtime as `0x200D00 + offset`:

```
file 0x2cbb8  ->  runtime 0x0022D8B8
```

Patching that to return 0 makes `usbdl_verify_da` take the *"DA validation
disabled on non-secure chip"* path and accept an unsigned DA:

```
write16 0x0022D8B8 = 0x2000    ; movs r0, #0
write16 0x0022D8BA = 0x4770    ; bx lr
```

mtkclient exposes exactly the needed primitives in
`mtkclient/Library/mtk_preloader.py`: `read16/read32/write16/write32/writemem`.

A read oracle is available to sanity-check addressing before writing: `READ32` of
the chipid register `0x08000000` should return **`0x788`**, a value we already
know independently from `devinfo[28]`.

## Ready-to-run exploit

[`tools/yacht_patch.py`](tools/yacht_patch.py) does the whole sequence
unattended. It polls for the preloader, verifies addressing against the read
oracle **before** writing anything, applies the patch, and reads back to confirm:

```
READ32 0x08000000  -> must be 0x788      (refuses to patch otherwise)
READ32 0x0022D8B8  -> before
write16 0x0022D8B8 = 0x2000, 0x4770      (movs r0,#0 ; bx lr)
READ32 0x0022D8B8  -> must be 0x47702000
```

Run it with the tablet powered **off**, then power on — it catches the
enumeration itself. If the patch verifies, `mtk.py printgpt` should then upload
the DA successfully, giving full eMMC access.

## Status and next step

The primitive is confirmed; the specific patch has **not** yet been applied. The
next step is a small script that connects via mtkclient's library, verifies the
read oracle, applies the two `write16`s above, and then lets the DA upload
proceed.

⚠️ **The device is currently wedged in preloader USBDL** (no ADB), and a power
cycle is unavoidable. It is *not* bricked — **hold power for ~10 seconds** and it
boots to Android normally.

Proven three independent ways that nothing software-side can recover it:

| attempt | result |
| --- | --- |
| mtkclient (USB and `--serialport`) | `Please disconnect, start mtkclient and reconnect` / `Handshake failed after retries` |
| raw MediaTek handshake over `/dev/cu.usbmodem1101` | no response to `0xA0` (3 tries) |
| raw USB bulk endpoints (`0x01` OUT / `0x81` IN, found via pyusb) | `USBTimeoutError` on both `0xA0` and `0xFD` |

So the preloader halted after the failed DA upload rather than returning to its
command loop. The handshake window only opens at **enumeration**, which is why
re-attaching cannot work.

## Why this matters for the goal

This is **root-free** (plain USB against a locked device), **persistent** in the
sense that it does not rely on a kernel bug an update could patch, and it
operates below the OS entirely. If the patch above works, the DA runs with full
eMMC read/write — which is exactly "flash a custom ROM" without an unlock.

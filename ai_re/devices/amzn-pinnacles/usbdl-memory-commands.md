# The last root-free lead: preloader USBDL memory commands

**The preloader's USB download handler implements MediaTek's arbitrary memory
read/write commands.** If they are not gated on this device, that is arbitrary
memory access over USB with **no root, no BROM, and no Download Agent** — enough
to patch the preloader in RAM and take control of the boot chain.

This is the only root-free path still standing, and it is testable.

## What the dispatcher accepts

Recovered from the USBDL command switch at `~0x4b40`–`0x4d20` in
`yacht_preloader.bin`:

```
0x80  0xa1 WRITE16   0xa2 READ32   0xc4  0xc5  0xc6  0xc7
0xd0 SEND_DA   0xd1  0xd2  0xd3  0xd4   0xd5 JUMP_DA
0xd7 SEND_CERT   0xd8 GET_ME_ID   0xdb   0xe1   0xe7
0xf0   0xfb   0xfc   0xfd   0xfe
```

**`0xa1` (WRITE16) and `0xa2` (READ32) are the significant pair** — they are the
same primitives the kamakiri/amonet family uses against the BootROM, here
implemented in the *preloader*.

That matters because **preloader USBDL (`0e8d:2000`) is a different mode from
BROM (`0e8d:0003`)**, and the `EFUSE_Disable_BROM_CMD` fuse we confirmed set
(bit 8 of `0x946`) disables the **BootROM** command handler — not the
preloader's. R0rt1z2's guidance for exactly these devices is *"find an exploit in
the preloader, which is still accessible."*

## Why it is plausibly reachable

`Tool connection is unlocked` (`0x18fca`) is printed **unconditionally**, right
before `bldr_handshake`:

```
0x18fb2  ...'before bldr_handshake'
0x18fca  ...'%s Tool connection is unlocked'   <- not inside any branch
0x18fd2  bl   #0x172cc                          ; bldr_handshake
0x18fd6  cmp  r0, #1
0x18fd8  bne  #0x18fe4
0x18fda  movs r2, #2 ; str r2, [r3]             ; tool connected -> state = 2
```

So it is a **status line, not a gate** — the preloader offers the tool connection
on every boot. Whether the *memory* commands within it are additionally gated
(by SLA, Serial Link Authentication) is the open question.

Evidence that SLA may not be enforced: the phrasing "unlocked" itself, and the
absence of any SLA challenge/response strings in the binary (the only related
strings are `sbc_enabled`, `daa_enabled`, and `invalid susbdl config`).

## What we know about the security state

`efuse 0x11f10060 = 0x946` (see [efuse-answer.md](efuse-answer.md)):

- **bit 2 = 1** → SBC enabled → **DA validation enforced** (so `SEND_DA` is
  useless to us)
- **bit 8 = 1** → BROM command handler disabled

The preloader also passes a security block to LK, whose layout is confirmed by
the boot-arg dumper at `~0x20208`:

```
+8  sbc_enabled     +9  daa_enabled
+a  rpmb_state      +b  prod_dev
```

`rpmb_state` and `prod_dev` match the Android properties `ro.boot.rpmb_state=2`
and `ro.boot.prod=1` exactly, which validates the layout. `sbc_enabled` and
`daa_enabled` are **not** surfaced as `ro.boot.*` properties, so they cannot be
read from Android — but the fuse read already answered SBC.

## The test

Entirely read-only, and it does not modify the device:

1. Power off. Enter **preloader USBDL** — hold the download key combo while
   plugging USB. Watch for **`0e8d:2000`** (preloader USBDL), which is the target
   here; `0e8d:0003` would be BROM and is expected to be dead.
2. With `mtkclient` attached in preloader mode, issue a **read**:
   ```
   mtk r boot_para boot_para.bin        # or
   mtk printgpt
   ```
   or drive `READ32` (`0xa2`) directly against a known-good address such as the
   chipid register `0x08000000`, which should return **`0x788`** — we already know
   that value from `devinfo[28]`, so it is a perfect oracle.
3. **If READ32 returns `0x788`, the memory commands are ungated** and this is the
   path: use WRITE16/READ32 to patch the running preloader and bypass the DA
   signature check entirely, or simply to read and write eMMC.
4. If the command is rejected or times out, SLA is enforced and this closes too.

Note the fuse disables the *command handler*, not USB enumeration — so the device
appearing on the bus proves nothing. Only a successful command with the expected
value does.

## Why this is worth doing

It is the only remaining route that is genuinely **root-free**, **persistent**,
and **not dependent on an exploit an update could patch**. And unlike the
force-BROM-recovery path in [brom-recovery.md](brom-recovery.md) — now a
confirmed brick — **this test writes nothing and risks nothing**. A rejected
command leaves the device exactly as it was.

The only cost is physical: someone has to hold the button combo. The key mapping
for the download combo is not yet resolved (`download keys are pressed` exists in
the preloader, and the force-recovery routine uses key id 0 via the helper at
`0x1845c`, which bounds ids at `0x47`).

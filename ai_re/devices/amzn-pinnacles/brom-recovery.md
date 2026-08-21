# The preloader has a built-in "force BROM download recovery"

**Found in `yacht_preloader.bin` at `0x24f84`–`0x2504e`.** Amazon's preloader
contains a deliberate service-recovery path that **destroys itself** to force the
BootROM into USB download mode.

This is the most promising route to flashing found so far — and the most
dangerous. Read the whole file before acting on it.

## What it does

The resolved string sequence, in execution order:

```
[%s] Start checking (1500 ms)
[%s] Key is not detected, wait for 1500ms
[%s] Key is detected
'preloader'                                   <-- target partition
[%s] Write image fail for seek offset 0x%x
[%s] Force brom download recovery success
```

The code path (`0x24fd0`–`0x2503a`):

```
0x24fb2  movw r0, #0x5dc        ; 1500 ms
0x24fb6  bl   #0x25560          ; wait
0x24fbc  bl   #0x1845c          ; re-check the key
0x24fec  bl   #0x24f14          ; get buffer
0x24ff2  mov.w r2, #0x800       ; 2048 bytes
0x24ff8  bl   #0x253b8          ; fill it (zeros)
0x25004  movs r2, #0            ; seek offset 0
0x2500c  bl   #0x1f5c0          ; WRITE to "preloader", 0x800 bytes at offset 0
0x25010  cmp.w r0, #0x800       ; all bytes written?
0x2502a  ...                    ; -> "Force brom download recovery success"
```

**It zeroes the first 2048 bytes of the `preloader` partition.** That destroys
the `EMMC_BOOT` header and GFH, so the BootROM finds nothing loadable and falls
through to USB download mode (USBDL). It is intentional self-bricking as a
recovery mechanism — the standard MediaTek service technique, shipped by Amazon
as a supported path.

## The trigger

Entry at `0x24f84`:

```
0x24f86  movs r0, #0            ; key id 0
0x24f88  bl   #0x1845c          ; is it pressed?
0x24f8c  cbnz r0, #0x24f92      ; no -> return 0
0x24f92  ldrb r4, [global]
0x24f98  cmp  r4, #0
0x24f9a  bne  #0x24f8e          ; a global byte must be 0
0x24f9c  bl   #0x249d0          ; a further condition must be nonzero
0x24fa2  beq  #0x24f8e
         ...wait 1500 ms, re-check the key, then wipe
```

So: **hold key id 0 through a 1500 ms confirmation window at power-on**, with two
additional internal gates satisfied. The key helper at `0x1845c` bounds the id at
`0x47`, i.e. it indexes a keypad/GPIO table — the physical mapping of id 0 is not
yet resolved. A related string, `download keys are pressed`, exists elsewhere in
the preloader.

## Why this matters — and the tension it exposes

The whole investigation has been running on the assumption, inherited from
community reports, that **BROM download is fused off** on 2020+ Amazon devices
(`EFUSE_Disable_BROM_CMD`). That assumption is what made `mtkclient` look
hopeless.

**But Amazon shipped a recovery feature whose only purpose is to drop this
device into BROM download mode.** A path that wipes the preloader is worthless —
worse, it is a permanent brick — unless BROM USBDL actually works on this
hardware. Its presence is real evidence that Amazon expects BROM recovery to be
reachable here.

That does not *prove* the fuse is unblown — the code may simply be inherited
MediaTek/Amazon common source retained across products where it no longer
applies. But it moves the BROM question from "almost certainly closed" to
"genuinely worth testing", which changes the priority ordering of everything.

Note also the preloader prints `sbc_enabled: 0x%x` and `daa_enabled: 0x%x`, and
carries `[EFUSE] sbc: %x`, `[EFUSE] sbc_key_hash is correct/incorrect`, plus
`[%s] invalid susbdl config '0x%x'`. **With UART, the device tells you its own
secure-boot and download-mode fuse state at boot.** That is the cheap, safe way
to answer the question.

## ⚠️ Do NOT trigger this yet

Wiping the preloader is **irreversible from software**. If `EFUSE_Disable_BROM_CMD`
*is* blown on this unit, the result is:

- no valid preloader → nothing for the BootROM to load
- BROM command handler disabled → no USB recovery
- **permanently, unrecoverably bricked** — no fastboot, no ADB, no BROM

There is no undo. The device becomes scrap.

## Correct order of operations

1. **Get UART.** Read `sbc_enabled` / `daa_enabled` / the `[EFUSE]` lines the
   preloader already prints at every boot. This answers the fuse question with
   zero risk and no modification.
2. **Probe BROM read-only** — power off, hold VolUp / VolDown / both, plug USB,
   watch for `0e8d:0003` (BROM) vs `0e8d:2000` (preloader USBDL). If `0e8d:0003`
   appears, issue a **read-only** `mtk printgpt` or `mtk r boot_para …`.
   Success means the command handler is alive.
   *(Remember: the fuse disables the command handler, **not** enumeration — the
   device may still appear as `0e8d:0003` while ignoring everything. Only a
   successful command proves it.)*
3. **Only if BROM commands demonstrably work** does the force-recovery path
   become a sane option — and even then it is a last resort, because the same
   access would already let `mtkclient` read and write flash directly, making
   self-bricking unnecessary.
4. Resolve the physical key mapping for id 0 before any attempt, so the trigger
   is not hit by accident.

## Also worth resolving

- `bl #0x249d0` — the second gate. If it checks something we control (an IDME
  flag, a boot mode), the trigger conditions may be reachable deliberately.
- The global byte at `0x24f96` that must be zero.
- `usbdl_verify_da` at `~0x4d7e` (`da_len (0x%x) is less than sig_len (0x%x)`)
  and `DA authenticated` at `~0x4e0a` — the Download Agent signature check. If
  BROM turns out to be reachable, this is the next thing to audit, since a flaw
  there would let an unsigned DA run.
- `Tool connection is unlocked` at `~0x18fca` — what state unlocks the tool
  connection, and whether it is reachable.

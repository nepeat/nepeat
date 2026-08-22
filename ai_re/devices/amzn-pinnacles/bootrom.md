# ⭐ We have the MT8183-family BootROM

**Acquired and independently verified 2026-08-21.** This is the root of trust for
our SoC, obtainable *without* touching our fused device — because the BROM is
**mask ROM, identical on every die of the same hwcode**. Our blown
`EFUSE_Disable_BROM_CMD` disables the command *handler*; it does not alter ROM
contents.

## The image

**`Zenofex/SoC-BootROMs` → `mediatek/mt6771.bootrom.bin`**
`https://raw.githubusercontent.com/Zenofex/SoC-BootROMs/master/mediatek/mt6771.bootrom.bin`

Added in [PR #11](https://github.com/Zenofex/SoC-BootROMs/pull/11) (merged
2023-12-31). PR body: *"Dumped from a Unihertz Titan using mtkclient. Original
file name assigned by the tool: `brom_MT6771_MT8385_MT8183_MT8666_788.bin`"* —
mtkclient's chip-name string for **hwcode 0x788**, exactly what our device
reports live (`HW code: 0x788`).

```
size    131072 (0x20000)
sha256  133ff75006686b3129c13630952de690829ec928c4603efb208e9381a08b98cc
```

Kept at `/tmp/brom8183/mt6771.bootrom.bin` — **not committed** (third-party ROM).

## Verified genuine, not a mislabelled blob

Checked independently here against offsets that two *separate* projects derived
from their own 0x788 dumps (mtkclient `src/stage1/targets/mt6771.h`,
MTK-bypass `exploit_mt6771/device.c`):

| offset | symbol | bytes | verdict |
| --- | --- | --- | --- |
| `0x4DAE` | `send_usb_response` | `30 b5 00 23` | `push {r4,r5,lr}` ✔ |
| `0xDDCE` | `usbdl_put_dword` | `2d e9 f8 4f` | `push.w` ✔ |
| `0xDE9E` | `usbdl_put_data` | `10 b5 06 4a` | `push {r4,lr}` ✔ |
| `0xE2D0` | `brom_register_access` | `2d e9 f0 41` | `push.w` ✔ |
| `0xDEBC` | `usbdl_ptr` (data) | `0x00102870` | SRAM pointer ✔ |

Six independently-published offsets all land correctly. Plus content strings
`EMMC_BOOT`, `UFS_BOOT`, `TIMESTMP`, `USBDLDBG`,
`[USBDL] Waiting for start cmd over 1 min ...`.

## Layout facts

- **Mapped at `0x00000000`.** First 16 bytes are an ARM (A32) vector table:
  `08 00 00 ea` (`b 0x28`) then `fe ff ff ea` (`b .`) ×3. Body is Thumb-2.
- **Window 0x20000 (128 KB); populated 0x0–0x17FFF (96 KB)**, zeros after —
  confirmed complete, not truncated.
- BROM SRAM working area is separate at `0x00100000+`: `brom_payload_addr
  0x100A00`, `ctrl_buffer 0x00102A80`, `meid 0x102B38`, `socid 0x102B48`,
  blacklist `0x102834` / `0x106A60`.

## What is already mapped here

**BROM command dispatch loop at `0xEBE8`** (mtkclient's `cmd_handler=0xEBE9`),
reached from a single caller at `0xD76A`. It reads a command byte via `0xDCE8`,
echoes it, and dispatches on `0xDC`, `0xD4`, `0xD0`, `0xD1`, `0xD2`, `0xD3`,
`0xD5`–`0xDB`, `0xA2`, `0xC8`, `0x88`.

**The eFuse accessor family** — each doing `ldr r0,[r0,#0x60]` then
`ubfx r0,r0,#N,#1`, reading the same `0x11f10060` already confirmed on-device:

> **⚠️ Correction:** an earlier revision of this file listed these entries **4
> bytes too low** (e.g. `0x7ABE` for bit 8). Those addresses are *mid-instruction*
> — `0x7ABE` disassembles as `lsls r0,r0,#3 ; bx lr`, the tail of the previous
> stub. That off-by-4 is the entire reason the "no callers anywhere" puzzle
> existed: the searches were run against addresses that are not function entries.
> Corrected and re-verified:

| stub | bit | meaning |
| --- | --- | --- |
| `0x7B40` | 0 | — |
| `0x7AA4` | 1 | `sbc_enabled` |
| `0x7AAE` | 2 | `daa_enabled` |
| `0x7AB8` | 3 | — |
| `0x7A60` | 4 | — |
| `0x7A6A` | 5 | — |
| `0x7B1E`* | 7 | — |
| **`0x7AC2`** | **8** | **`EFUSE_Disable_BROM_CMD` — blown on our unit** |
| `0x7B4A` / `0x7B54` / `0x7B5E` | 9 / 10 / 11 | — |

Literal pool: `0x7BC8 = 0x11F10000` (so `+0x60` is our register), plus
`0x7BCC = 0x11F10130` and `0x7BD0 = 0x11F10120` — **a second fuse/config bank**
whose stubs load at offset 0 rather than `+0x60`.

With the addresses corrected, every stub has ordinary `bl` callers. No computed
dispatch, no pointer tables — the puzzle was self-inflicted.

## ⛔ What bit 8 gates: EVERYTHING, with no pre-check window

`0x7AC2` has exactly one caller, `0xDEE0`, inside `0xDED8`, whose only caller is
`0xD6A2`:

```
0xDED8  push {r4,r5,r6,lr} ; r4=r5=r6=0
0xDEE0  bl #0x7AC2              ; EFUSE_Disable_BROM_CMD
0xDEE4  cbz r0, #0xDEEC
0xDEE6  movw r4,#0x8000
0xDEEA  movs r6,#1              ; "skip download mode" verdict
...
0xDF68  mov r0,r6 ; pop
```

and the decision:

```
0xD6A2  bl #0xDED8
0xD6A6  cbnz r0, #0xD6C6        ; nonzero -> skip both channels
        ... UART path -> 0xD6C4 b #0xD76A -> bl #0xEBE8 (cmd handler)
        ... USB  path -> 0xD76A            -> bl #0xEBE8
0xD79A  bl #0xD550              ; load bootloader from boot device
0xD7A4  bl #0xD346              ; jump to it
```

**There are exactly two paths to the command handler — `0xD6C4` (UART) and
`0xD76A` (USB) — and both are downstream of the single `cbnz` at `0xD6A6`.** It
is not a per-command filter and not USB-only; it removes both channels at once.

**No pre-check window exists.** The full path from reset is
`0x0 → 0x28 → 0xA0 (blx) → main 0xD118 → 0xD67A → 0xD6A2`. `0xDCAA`, the only
call before it, merely installs a vtable of channel putc/getc pointers. No
handshake (`0xDB74` UART / `0x3890` USB), no `0xDCE8` (get command byte), and no
USB controller init runs first. **On a bit-8-blown die, no host byte is ever
read, echoed, or buffered before the fuse is consulted.** Clean negative — not a
race, not a late check.

## The command handler bit 8 protects is itself hardened

Even reaching it would not have handed over an easy primitive. Both the read
funnel (`0xE216`) and write funnel (`0xE116`) perform, in order: zero-length
reject, alignment check, a **multiplication-overflow guard** (`cmp r5,#0x40000000`
for 32-bit / `#0x80000000` for 16-bit) *before* computing byte length, then range
validation via `0xE066`. The range primitive `0xD130` has the **carry
hardening**:

```
0xD130  cbz r1 -> error 0x706A      ; len == 0
0xD136  adds r6, r0, r1             ; start + len
0xD138  blo  0xD146                 ; no carry -> proceed
0xD13A  movw r0,#0x706b             ; wraparound -> error
```

the same newer form as the preloader. Three blacklist tables are consulted on
every access. mtkclient's `0x102834` and `0x106A60` are **entry counts**, not the
tables themselves (`[0x102830]=0x00012614` table, `[0x102834]=10` count;
runtime table at `0x1069E0` with count at `0x106A60`).

**Verdict: no unbounded access, no integer-overflow bypass, no off-by-one.**

## SBC and DAA at BROM level

**SBC (`0x7AA4`, six callers)** is the real restrictor. When set, BROM reads are
confined to three windows — `11F10000–11F11000`, `10007000–10008000`,
`1001A080–1001A110` — and writes to the latter two. (Compare the preloader's own
whitelist, measured live: read `{0x11f10000/0x1000, 0x10007000/0x1000,
0x1001a080/0x4}`, write the latter two. Same policy, one layer down.)

**DAA (`0x7AAE`) does essentially nothing at BROM level** — only two callers,
neither in the memory path; it is one of three OR'd inputs to a generic "some
secure mode is on" predicate at `0x6DFC`. The DA-authentication enforcement we
mapped in the preloader is **not** mirrored in the BROM. Worth recording as a
clean negative.

### ✅ Second restrictor confirmed SET on our device

`0x11F10130` **bit 10** (stub `0x7ACC`) **short-circuits SBC** — when set, reads
and writes are restricted regardless of SBC. We already had this value without
knowing it mattered: the preloader devinfo table maps entry 11 to reg
`0x11f10130`, and our live `atag,devinfo` read gave **`devinfo[11] = 0x00000460`**.

```
0x460 = 0b0100_0110_0000  ->  bit5=1, bit6=1, bit10=1
```

**Bit 10 is set.** So the second, independent restrictor is enabled on this unit
too. Anyone modelling "SBC off ⇒ unrestricted BROM" must check `0x11F10130` as
well.

## Is the ROM readable?

**BROM's own commands refuse it.** The range `00000000–00018000` — exactly the
populated ROM extent — is a blacklist entry under mask `0x2`, inside the `0x19F`
deny mask queried at `0xE070`. That is why mtkclient dumps this ROM with an
injected SRAM payload rather than `READ32`.

**But there is no hardware lockdown on the handoff path.** The exit is
`0xD79A → 0xD550 → 0xD346`, and `0xD346` is a bare indirect jump
(`ldr r2,[pc]; ldr r2,[r2]; ldr r2,[r2,#4]; bx r2`) — no ROM-disable write, no
MPU programming. **The BROM does not unmap or access-control itself before
handing off.** So the blacklist is evidence the ROM *is* physically readable from
a non-BROM context (you do not blacklist an address that faults), and our
preloader's refusal of `0x0` is a *software* filter. Whether a read succeeds
under the preloader is untested.

## Open leads

1. **`'RESV'` at SRAM `0x100000`** (`0xD206`–`0xD22E`): the BROM preserves
   `0x100000–0x100040` across warm resets if the magic is present, and both
   sub-ranges are separately blacklisted from command writes — i.e. protected
   *because* they persist. A warm-reset persistence channel.
2. **`0xD290` structure mismatch**: it writes `start→[0x102838]`,
   `end→[0x10283C]`, `1→[0x102848]`, but the consumer at `0xD278` reads a
   12-byte `{mask,start,end}` stride from base `0x102840`. Those do not line up.
   Either a mis-resolved literal or a genuine mismatch — **not** claimed as a
   bug, but the most interesting loose thread in the range-check machinery.
3. `globals[7]`/`globals[8]` at `[0x106A70]`/`[0x106A74]` drive a
   `0xC975E033` magic and `'R'`/`'U'` override bytes that can *disable* download
   channels (never enable). Where they are populated is unmapped.

## Bottom line for the goal

**The BROM avenue is closed, and now for a proven reason rather than an assumed
one.** Bit 8 is a hard, early, single-branch kill of both download channels,
taken before any port is initialised and before a single host byte is read — so
there is no window to attack. And the handler it protects is properly
bounds-checked, so reaching it would not have yielded a memory primitive anyway.

The one thing this *opens* is the preloader-side question: the ROM is not
hardware-locked at handoff.
## Why this matters

The root of trust is now reversible **offline**, with no risk to the device and
no dependence on a donor unit. Everything below LK — DA authentication, the SLA
challenge, the download-mode entry decision — can be studied directly.

## Also available from the same repo (for cross-SoC diffing)

131072-byte dumps for mt6572, mt6580, mt6739, mt6750, mt6753, mt6761, mt6765,
mt6768, mt6789, mt6855, mt6877, mt6893, mt6895, mt8127, mt8135, mt8163, mt8167,
mt8173, mt8186, mt8382v, mt8735v/w.

## Caveat, stated honestly

The die dumped was an **MT6771** (Unihertz Titan), not literally an MT8183. All
four names (MT6771 / MT8385 / MT8183 / MT8666) report hwcode `0x788` and share a
single mtkclient `Chipconfig` with one set of BROM-internal offsets, and
mtkclient's 0x788 path is used against MT8183 hardware in the wild — so the ROM
is expected identical. A BROM *revision* within hwcode 0x788 is **not yet ruled
out**.

**Cheap way to close it:** our preloader handshake already prints `HW subcode`,
`HW Ver`, `SW Ver`, and those are **not currently recorded** in these notes. A
published MT6771 log (mtkclient issue #117) shows `subcode 0x8A00 / HW Ver
0xCA00 / SW Ver 0x0`. Capture ours on the next routine handshake and compare.

## Clean negatives (searched, does not exist)

- No `mt8183.bootrom.bin` / `brom_mt8183*` anywhere on GitHub.
- The widely-surfaced fork `arzam16/SoC-BootROMs` does **not** carry 0x788 —
  only the Zenofex upstream does.
- `bkerler/mtkclient` ships no BROM dumps; note mt8183/mt6771 are absent from
  its `missing_brom.txt`, implying the maintainer holds a dump privately but has
  never published one.
- amonet / kamakiri / MTK-bypass ship payloads and derived offsets, no ROM images.
- tinyhack's "Dissecting a MediaTek BootROM exploit" is MT6873, publishes no dump.

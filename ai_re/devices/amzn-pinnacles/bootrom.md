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

**The eFuse accessor family at `0x7A5C`–`0x7B68`** — the BROM's own copies of the
same stubs found in the preloader, each reading `[efuse_base + 0x60]` and
extracting one bit:

| stub | bit | meaning |
| --- | --- | --- |
| `0x7B3C` | 0 | — |
| `0x7AA0` | 1 | `sbc_enabled` |
| `0x7AAA` | 2 | `daa_enabled` |
| `0x7AB4` | 3 | — |
| `0x7A5C` | 4 | — |
| `0x7A66` | 5 | — |
| `0x7B1E` | 7 | — |
| **`0x7ABE`** | **8** | **`EFUSE_Disable_BROM_CMD` — the one blown on our unit** |
| `0x7B46` / `0x7B50` / `0x7B5A` | 9 / 10 / 11 | — |

This is the same register (`0x11f10060`) and the same bit numbering already
confirmed on our device by direct read (`0x946`).

## Open question — handed to dedicated analysis

**None of these stubs has a direct `bl` caller, and none appears as a Thumb
function pointer (`addr|1`) anywhere in the image.** So either they are reached
by some other mechanism, or the BROM's real gating is inlined elsewhere and
these are out-of-line copies. Resolving *what bit 8 actually gates* is the
decisive question: it determines whether any BROM command survives the fuse, and
whether there is a window before the check.

Do **not** assume the fuse disables the whole handler until the gate is found —
that is exactly the kind of unverified inference this project has had to retract
repeatedly.

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

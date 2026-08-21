# Reversing our own LK — the unlock verdict, and a better way in

Ghidra + static analysis of `fw-partitions/dump/lk.img`, dumped from this device
with root on 2026-08-21. This supersedes the reference-binary work in
[lk-analysis.md](lk-analysis.md) — everything below is from **our** bootloader.

## Verdict on generating an unlock key: you can't

Not "probably not" — the bootloader's own contents settle it. Three facts, all
confirmed in our binary:

**1. The signed message format is exactly as the leaked UFBL source described.**
At offset `0x4b9fa`:

```
0x%08x%08x%08x
```

immediately followed at `0x4ba09` by the string `unlock_version`. That is
`sprintf("0x%08x%08x%08x", SoC_ID, HW_ID, unlock_version)` — three 32-bit values,
device-unique, with `unlock_version` acting as an anti-replay nonce that
`oem relock` rerolls.

**2. Six RSA-2048 public keys are compiled into LK**, and two of them sit
directly in the unlock code:

| # | offset | SHA-256 of SubjectPublicKeyInfo | adjacent strings |
| --- | --- | --- | --- |
| 0 | `0x473df` | `2af78db652e9f848…93a21b57` | *Common Kernel Signing **Production** CA* |
| 1 | `0x47939` | `3ab387e5cd47acca…d25dae86` | *Common Kernel Signing **Engineering** CA* |
| **2** | **`0x4b8d4`** | `de6344d427a656a1…2e4db500` | ***unlock signature verify failed***, *Only usr_flags…* |
| **3** | **`0x4baec`** | `851766d2880d4dd3…8f12e704` | ***unlock signature verify failed***, *correct signature, but update idme failed* |
| 4 | `0x57384` | `85d7f6f743faae54…07fbe2b9` | display / panel |
| 5 | `0x574ac` | `be68a42f3f32acc4…fc66e5c8` | *vpu anti-rollback version mismatch* |

Keys **2** and **3** are the unlock pair — permanent (`unlock`) and temporary
(`tucert`). Verification is LibTomCrypt RSA-PSS; the build paths
(`.../libtomcrypt/src/pk/rsa/rsa_verify_hash.c`,
`.../pkcs1/pkcs_1_pss_decode.c`) are still in the binary.

**3. The error paths spell out the flow:**

```
signature length error, wrong signature file? do nothing!
unlock signature verify failed, do nothing!
correct signature, but update idme failed
write tucert failed!
```

Two fastboot entry points exist — `flash:unlock` (`0x4b683`) and
**`flash:tucert`** (`0x4b690`), the temporary-unlock cert path.

**Conclusion:** producing a valid `unlock.bin` or `tucert` requires the RSA-2048
private key matching key 2 or 3. That key is Amazon's, has never leaked, and
cannot be derived from the public key. **There is no way to generate an unlock
code for this device.** Anything that claims otherwise is either a signature
forgery (infeasible) or a bug in the verifier (see below).

### On the BootROM

Worth restating since it was part of the ask: **the BootROM does not
participate in the unlock decision at all.** BL1 (BootROM) verifies the
preloader; BL2 (preloader) verifies LK; **only LK** reads IDME and decides lock
state. The MT8183 BootROM is also generic MediaTek silicon — identical on every
MT8183 part — so there is nothing device-specific to learn from dumping it, and
`mtkclient dumpbrom` needs the very BROM access that Amazon fuses off anyway.
Skipping it costs us nothing.

## The better way in: IDME flags, bypassing fastboot entirely

This is the actually useful result.

LK contains:

```
[SELINUX] set to permissive mode by dev_flags
[DM-VERITY] verify off by fos_flags
Only usr_flags can be set for a locked device
```

So `dev_flags` makes SELinux permissive **at boot**, and `fos_flags` turns off
**dm-verity** — together, most of what running a modified system actually
requires. No unlock needed.

The gate is *"Only usr_flags can be set for a locked device"*. Critically, that
check lives in the **`oem flags` fastboot handler** — it guards the *fastboot*
path to writing IDME. **We have root, so we don't need fastboot.**

### The IDME structure is unsigned, and we've decoded it

From our `idme_boot1.img` (eMMC boot1 = `mmcblk0boot1`):

```
header:  "beefdeed" "2.1" \0 <count=0x23>
entry:   name[16] | size[4 LE] | type[4] | 0x00000124[4] | value[size]
```

Verified against live values — `board_id` entry yields `0060001400000021`,
`serial` yields `G002G402413303RL`, both matching `/proc/idme`. The flags are
stored as **plain ASCII**:

| field | entry offset | **value offset** | current |
| --- | --- | --- | --- |
| `dev_flags` | `0x2274` | **`0x2290`** | `"0"` (`0x30`) |
| `fos_flags` | `0x2298` | **`0x22b4`** | `"0"` (`0x30`) |
| `usr_flags` | `0x22bc` | `0x22d8` | `"0"` |

**LK validates only the magic number** — its only integrity strings are
*"Idme data not present: magic number error"*. No CRC, no checksum, no
signature over the field table. Only the `unlock_code` blob inside IDME is
signature-checked; the rest of IDME is not.

And the eMMC boot area is writable from root — `force_ro` is `1` on both
`mmcblk0boot0` and `mmcblk0boot1`, which is the standard *software* protection
that root can clear.

### So the proposed change is

```sh
echo 0 > /sys/block/mmcblk0boot1/force_ro
# write ASCII '1' (0x31) at byte 0x2290  -> dev_flags = 1
# write ASCII '1' (0x31) at byte 0x22b4  -> fos_flags = 1
echo 1 > /sys/block/mmcblk0boot1/force_ro
```

Two single-byte writes. If LK honours them, you get SELinux permissive and
dm-verity disabled at boot, persistently, on a still-locked bootloader.

> ⚠️ **Not yet attempted — this needs a decision.** It is a write to the eMMC
> boot area, which holds the serial, MACs and calibration alongside the flags.
> We hold a full backup (`idme_boot1.img`, hashed in
> [`dumps/partition-manifest.txt`](dumps/partition-manifest.txt)), and the
> writes are two bytes at known offsets, so the change is small and revertible
> *provided* the device still boots. Unknowns: whether LK cross-checks the flags
> against lock state before honouring them, and whether nonzero `dev_flags` on a
> production-signed unit triggers the *"Authentication failed on engineering
> device with production certificate"* path. Both are answerable by reading the
> Thumb code around those strings before touching anything.

## Tooling notes

- The LK container is **MTK v1.0**: header magic `0x58881688`, name `lk`,
  payload 603,920 bytes starting at offset `0x200`, block size `0x200`. Not the
  v2.0 multi-partition container the guides assume, so `lkpatcher` isn't needed —
  a plain `dd` from 512 gets the payload.
- **The payload is Thumb-2, not ARM**, despite an ARM vector table at offset 0.
  Ghidra imported as `ARM:LE:32:v7` decodes the body as garbage; the giveaway is
  `46xx`/`b0xx`/`f0bd`/`00bf` byte patterns. Any further function-level work
  needs TMode set, or an external Thumb disassembler.
- String references are **PC-relative (ADR)**, not absolute pointers, so the
  load base doesn't matter for xref analysis. Base-scoring by self-pointers gave
  only a plateau around `0x4478xxxx` — no sharp answer, and none needed.

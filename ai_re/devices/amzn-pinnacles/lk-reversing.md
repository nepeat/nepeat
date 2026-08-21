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

## Verification attempt: how far it got, and where it stopped

The plan was to read the code around the flag strings and confirm LK honours
`dev_flags`/`fos_flags` on a locked production unit. **That verification is
incomplete** — instruction-level analysis is blocked. Recording the dead ends so
nobody repeats them.

**What blocked it.** String references in this payload resolve to *neither*
PC-relative `ADR` nor absolute literal-pool pointers:

- Decoded every Thumb `ADR.N` (T1) and `ADR.W` (T2/T3) encoding in the image and
  computed targets — **zero** hit any of the seven target strings.
- Searched for absolute literals at 20 known string offsets across every
  plausible base. Best consensus was **7 of 17 strings at an unaligned base**
  (`0x44739150`), i.e. noise. A correct base would hit nearly all of them.
- Literal-value distribution *does* point at an image around `0x44780000` (the
  `0x4478` prefix dominates at 1433 hits, tailing through `0x4479`–`0x447c`,
  consistent with a `0x93710` span), but no base in `0x44700000–0x44840000` at
  4-byte granularity explains the string references.

The likely explanation: **`lk.img` is not one flat blob.** There are extra MTK
header magics (`0x58881688`) at `0x126d0`, `0x30ba4`, `0x33208`, `0x3f9a0`, GFH
magics around `0x325b4`, and the strings fall into three distinct clusters
(`0x47xxx–0x4c000`, `0x57xxx`, `0x76xxx`) each with its own nearby RSA keys. So
this is several sub-images concatenated, each with its own load base — the code
for the `0x76xxx` SELinux/dm-verity strings is probably in a different sub-image
than the one starting at offset `0x200`.

Also note **Ghidra was misleading here**: with the payload imported as
`ARM:LE:32:v7` it produced apparently-real xrefs (e.g. `amzn_verify_unlock` at
`0x48054` referenced from `0x1b1a`). Those were **artifacts of decoding Thumb-2
as ARM** — the byte patterns are plainly Thumb (`46xx`, `b0xx`, `f0bd`, `00bf`).
Don't trust xrefs from that program.

## What the evidence *does* support

Short of instruction-level proof, the **string locality** is meaningful, because
the linker groups strings by translation unit:

- The gate — `Only usr_flags can be set for a locked device` (`0x4b75f`) — sits
  in the same tight cluster as the fastboot command strings: `oem flags`
  (`0x4b679`), `flash:unlock` (`0x4b683`), `flash:tucert` (`0x4b690`),
  `%s: Assuming fos_flags…` (`0x4b78d`), `%s: Managed to set flags.`
  (`0x4b85d`), `oem flags [<type>: <modifier>] <value>` (`0x4b878`). That is the
  **`oem flags` command handler**.
- The consumers — `[SELINUX] set to permissive mode by dev_flags` (`0x76a80`),
  `[SELINUX] enforced by dev_flags`, `[DM-VERITY] verify off by fos_flags`
  (`0x769bd`) — are in a **completely different cluster**, ~0x2b000 away, with no
  lock-state string anywhere near them.

So the lock check lives in the *setter* (fastboot), and the *consumers* read the
flag values and branch on them, with no lock-state string in their vicinity.
That is consistent with the theory — root writing IDME directly bypasses the
gate — but it is **circumstantial, not proven**.

## Recommended next step: a low-risk empirical test

Rather than keep fighting the disassembly, test the cheap half first:

**Write `dev_flags = 1` only. Leave `fos_flags` at 0.**

- `dev_flags` only affects SELinux mode. If LK honours it, `getenforce` reports
  `Permissive` after a clean reboot — a clear, readable signal.
- It does **not** touch dm-verity, so `/system` integrity checking stays on and
  the boot path is unchanged.
- One byte at `0x2290` of `mmcblk0boot1`, with a full backup of the region and
  root available to revert.
- If LK ignores it, we learn that for free and `fos_flags` is almost certainly
  the same.

Only if that works is it worth considering `fos_flags`.

## Disassembly: solved, partially

Later work cracked most of the earlier blockers. Recording the mechanics because
they cost hours to find.

**1. Use capstone in Thumb mode, not Ghidra.** `Cs(CS_ARCH_ARM, CS_MODE_THUMB)`
produces clean, obviously-correct output on this payload:

```
0x31232  bl       #0xdb14
0x31238  cbz      r0, #0x31240
0x3123a  ldr      r0, [pc, #0xa0]
0x3123c  add      r0, pc
```

**2. The first sub-image is linked at base 0.** Confirmed by a genuine literal
pool at `0xfc64` holding `0x00048054`, which is exactly the file offset of the
`amzn_verify_unlock` string, alongside neighbouring pointers into the same
string block. So for *this* blob, literal value == file offset.

**3. String references are PC-relative GOT-style, not absolute.** The idiom is:

```
ldr rX, [pc, #imm]     ; load an OFFSET from the literal pool
add rX, pc             ; add current PC to get the final address
```

so `target = literal + pc_of_add + 4`. **This is why every absolute-pointer
search failed**, and why the earlier "no base explains the references"
conclusion was misleading — the references are real, just computed.

**4. The image is multi-blob with differing bases.** The `0x48xxx` cluster
(amzn unlock strings) resolves against base 0, while the `0x76xxx` cluster
(`[SELINUX]`, `[DM-VERITY]`) does not — those belong to a different sub-image.
Consistent with the extra MTK header magics at `0x126d0`, `0x30ba4`, `0x33208`,
`0x3f9a0`.

**What is still missing:** automated xref recovery. Linear Thumb disassembly
desyncs through the interleaved data/literal regions, so naive `ldr`/`add pc`
pairing across the whole image produces nothing usable. Doing this properly
needs real function-boundary recovery — walk from known entry points and
disassemble along control flow, rather than linearly. That is the remaining
work, and it is a normal (if unglamorous) reversing job now that the decoding
and addressing are understood.

A confirmed useful anchor to start from: the literal pool at `0x312c0` holds
pointers to `t_unlock_cert` and `"Verify temp unlock cert fail, ret = %d"`, and
the function body immediately before it (`0x31200`–`0x312b2`) disassembles
cleanly. That is the temp-unlock cert path.

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

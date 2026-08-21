# Bootloader unlock — feasibility

**Current verdict: not feasible by any published method, and the most promising
lead has now been tested and failed.** This file records why, so nobody
re-treads it.

Research 2026-08-21 plus a live fastboot test on the device. Raw output in
[`dumps/fastboot-getvar.txt`](dumps/fastboot-getvar.txt).

## Correction: dumping the BootROM does almost nothing here

Worth stating plainly, because it was an appealing idea and it's wrong.

**The BootROM does not participate in the unlock decision.** The chain of trust
splits the responsibilities cleanly:

| Stage | Is | Verifies | Knows about unlock? |
| --- | --- | --- | --- |
| BL1 — BootROM | mask ROM in the SoC | preloader, via SBC against a key hash burned in eFuse; gates DAA/SLA in download mode | **No** |
| BL2 — preloader | `mmcblk0boot0` | the bootloaders image — for LK v2.0 that's `lk` + `bl2_ext` + `aee` + dtbs, each with its own `cert1`/`cert2` | **No** |
| **BL3 — LK** | `mmcblk0p5` | reads IDME, runs `amzn_unlock_verify` / `amzn_verify_temp_unlock_code`, sets lock state, then verifies boot via AVB2 | **Yes — only stage that does** |

Amazon's unlock logic is pure LK code. The BootROM's entire remit is "is this
preloader signed by the key whose hash is in my fuses" — it has never heard of
`unlock.bin`, IDME, or `t_unlock_cert`.

Two further reasons it's not worth chasing: the MT8183 BootROM is **generic
MediaTek silicon**, identical on every MT8183 part (including MT8183
Chromebooks and the pre-fuse Fire HD 10 2019), so it needn't come from this
device; and mtkclient's `dumpbrom` **requires a working BROM command handler
plus an exploit payload** — i.e. it is gated on the very access the e-fuse
removes. It's a consequence of winning, not a way to win.

So of the three dumps: **flash matters enormously** (specifically `lk`), the
**bootloader IS the flash dump**, and **BootROM is a footnote**. The good news
is that the LK dump doesn't depend on the e-fuse question at all if the firmware
can be obtained another way — which decouples the two priorities.

## Tested: `ro.oem_unlock_supported=1` is not a real policy difference

This was the one genuine anomaly — retail Fire tablets ship it as `0`, this
non-retail SKU ships `1` — so it was the top lead. It does not survive contact.

ADB side: `settings get global oem_unlock_allowed` returns `null` (never set),
and there is no `sys.oem_unlock_allowed` property. The Settings app does still
contain the OEM-unlock UI entry.

Fastboot side, on the device in bootloader mode:

```
getvar:all                            FAILED (remote: 'the command you input is restricted on locked hw')
oem device-info                       FAILED (remote: 'the command you input is restricted on locked hw')
flashing get_unlock_ability           FAILED (remote: 'the command you input is restricted on locked hw')
oem lks                               FAILED (remote: 'the command you input is restricted on locked hw')
```

That is exactly the response documented for retail-equivalent locked Amazon
firmware. Amazon's LK runs an **allowlist** of permitted commands while locked —
only three of the individual getvars answered at all:

```
product              -> pinnacles
serialno             -> G002G402413303RL
max-download-size    -> 0x8000000        (128 MB)
```

Everything else — `version`, `unlocked`, `secure`, `hw-revision`,
`version-bootloader`, `partition-type:boot` — is restricted.

So `oem_unlock_supported=1` is best read as an **unscrubbed AOSP build flag** on
a low-volume SKU, not evidence of a relaxed unlock policy. Amazon's LK ignores
the AOSP unlock protocol entirely (it checks IDME, not `seccfg`, not
`get_unlock_ability`), so the property gates nothing here.

Fastboot itself is reachable and stable, and `fastboot reboot` returns the
device to Android with ADB authorization intact — the probe is safe to repeat.

## Definitive answer, from Amazon's own bootloader source

A leak of Amazon's **UFBL** (Universal Fire BootLoader) source settles the
mechanism exactly, replacing the inference below.

**Lock state is the IDME `unlock_code` field, stored in eMMC Boot Partition 2**
— Linux `mmcblk0boot1`, mtkclient `--parttype boot2`. Not `seccfg` (Amazon has
no such partition), and not primarily RPMB.

The field holds a **256-byte RSA-2048 PSS / SHA-256 signature** over:

```c
sprintf("0x%08x%08x%08x", SoC_ID, HW_ID, unlock_version)
```

verified with **LibTomCrypt** against a **product-specific Amazon public key
compiled into LK** as `UFBL_UNLOCK_PUBK_<PRODUCT>`. The relevant routines are
`amzn_target_is_unlocked`, `amzn_check_unlock_status`, and
`idme_get_var_external`.

**This resolves the `unlock_version` mystery.** Our 8 non-empty bytes
(`7ebd9a960c71a100`) are the **anti-replay nonce** — it's part of the signed
message, and `cmd_oem_relock` rerolls it to fresh randomness on relock,
deliberately invalidating any previously issued signature. So a cert is bound
not just to the device but to the *current* nonce.

**So: rewriting IDME is insufficient — but not because of RPMB.** It's
insufficient because those bytes are a signature over device-unique data checked
against a private key Amazon holds. Arbitrary bytes just fail `rsa_verify_hash`.
RPMB (`rpmb_state=2`) is a *secondary* anchor, gating the temp-unlock reboot
counter and its HMAC.

That makes `lk.bin` the single highest-value read-only dump: it carries the
embedded `UFBL_UNLOCK_PUBK_<PRODUCT>` modulus and the verification routines. The
realistic attack surface is a **length/parse bug in `idme_get_var_external`, or
an inverted/ignored PSS return value** — not the cryptography.

### `kb` / `dkb` — identified, and they are secrets

Not Amazon inventions; standard MediaTek, accessed via MTK's `kisd.te` (Key
Installation Service Daemon) and `hal_drm_widevine.te`. **`kb` is the
Widevine/attestation keybox, `dkb` the device keybox.** Amazon's own IDME table
defines `KB` (5120 bytes) and `DKB` (1024 bytes) as literal *backups* of those
partitions — which is why `/proc/idme/KB` returns a `KBPFH…` header here, and
why the empty `DKB` means the device keybox was never derived or was cleared.
`keys` (p3) remains unconfirmed.

> ⚠️ **`kb`, `dkb`, `keys` and any raw boot2/IDME dump are per-device secrets.**
> They must stay local and out of git — the device `.gitignore` now blocks them.
> Commit hashes, not bytes. (The committed `dumps/idme.txt` was checked: `KB`
> yielded only a 5-byte header, `mac_sec` was permission-denied, and the
> `bt_mfg`/`wifi_mfg` blobs are RF calibration tables, so no key material is in
> the repo.)

### mtkclient traps, if BROM ever opens

- **The boot partitions are off by one.** `mmcblk0boot0` = `--parttype boot1`
  (preloader); `mmcblk0boot1` = **`--parttype boot2`** (IDME, incl.
  `unlock_code`). Read the latter with
  `mtk r idme idme_boot2.bin --parttype boot2`. When `--parttype != user`,
  mtkclient ignores the partition *name* entirely and dumps the whole hardware
  partition, so the name is a dummy label.
- `mtk rl <dir> --parttype boot1` finds no GPT, **silently falls back to
  `--parttype user`**, and hands you a mislabelled full user-area image.
- `mtk rf rpmb.bin --parttype rpmb` returns **zeros** — RPMB isn't a block
  device. Only `mtk da rpmb r rpmb.bin` speaks the real protocol.
- **`da seccfg unlock` fails safe here** (it scans the user GPT for `seccfg`,
  finds none, aborts) — **but it has an interactive fallback prompting for
  `v3`/`v4` that writes a *fresh lock state*. Never answer that prompt.**
- `dumpbrom` produces a genuine complete 128 KiB BootROM, not partial output —
  the only obstacle is the fuse.

⚠️ **Unresolved discrepancy:** the two research passes disagree on the MT8183
hwcode — `0x788` vs `0x766`. Confirm against `brom_config.py` before relying on
either.

## How the unlock works — earlier inference, now superseded

*Kept because it was independently derived and mostly correct; the UFBL source
above is authoritative where they differ (notably boot1 vs boot2).*

Confirmed as Amazon-wide architecture across generations:

- Amazon does **not** use AOSP `fastboot flashing unlock` or the `seccfg`
  partition. Lock state lives in the **IDME** region — an `unlock` pseudo-
  partition inside **boot1**, not in the by-name table.
- The permanent path is `fastboot flash unlock unlock.bin`, with an
  **Amazon-signed** payload.
- The signed value is **device-bound**, derived from the eMMC manufacturer id
  (`/sys/block/mmcblk0/device/manfid`) and production serial
  (`.../serial`) — both `Permission denied` to our unprivileged shell. *(That
  exact format is sourced from a Fire OS 4-era thread, so treat it as inferred
  by analogy; the `unlock_code` IDME field name has survived unchanged.)*
- Verification is **RSA-PSS in LK** — bootloader strings reference a 256-byte
  signature (RSA-2048) and a *"fail to pass RSA-PSS verification"* failure
  path. The routine is `amzn_unlock_verify` / `amzn_verify_unlock`.
- The **Temporary Unlock** path (2019+) matches our `t_unlock_*` fields
  one-for-one: `amzn_get_temp_unlock_idme_data` / `_cert` / `_code`, verified by
  `amzn_verify_temp_unlock_code`, with the runtime string *"Device is
  temporarily unlocked, %d reboots remaining"*.

### The key structural point

Unlock state is stored somewhere **forgeable** (IDME/boot1, plain eMMC, no
replay protection) but is **cryptographically validated on every boot** by LK.

`rpmb_state=2` turns out to be **anti-rollback**, not unlock state — the LK
strings around RPMB are all version-counter language (*"Anti-rollback block
initialized"*, *"Invalid anti-rollback state"*), none of it unlock language.

So the earlier worry in [customization.md](customization.md) that RPMB might
anchor the unlock state is **resolved: it doesn't.** But that changes nothing
useful, because the blocker was never storage — it's the signature. Getting
root and writing `t_unlock_code` yourself buys you nothing: it fails
`amzn_verify_temp_unlock_code` and you boot normally. **Write access to IDME is
worthless without Amazon's private key.**

The flip side: `rpmb_state=2` means anti-rollback is armed, which independently
kills any LK-downgrade idea.

## BROM is almost certainly fused off

The MediaTek BootROM path — which everything else depends on — is disabled on
2020+ Amazon hardware:

> The Fire HD 8/2020 blocks BROM access using a supplementary bit in
> `efuse_base+0x60` at position `0x100`. If set, the entire `cmd_handler` is
> skipped. The fuse is `EFUSE_Disable_BROM_CMD`.

Confirmed for the closest relative: Fire HD 10 11th gen (`trona`, also MT8183)
"has BROM access disabled, which prevents MTKClient from working." Reports of
mtkclient *succeeding* on MT8183 trace to the **2019** Fire HD 10 (`maverick`,
pre-fuse) and to early trona units — it is a **production-date-dependent fuse**,
and a 2022-04-01 preloader puts this unit well on the wrong side.

Important detail: the fuse disables the BROM **command handler**, not USB
enumeration. The device may still enumerate as `0e8d:0003` while ignoring every
command — **do not read enumeration alone as success.**

Even if BROM were open, mtkclient's `da seccfg unlock` is useless here, because
Amazon doesn't use `seccfg`. It would only ever buy eMMC read/write plus a
*tethered* patched-preloader boot — no computer, no boot, forever.

### Testing it non-destructively

1. Monitor `ioreg`/`lsusb`, plug in powered-off while holding VolUp, then
   VolDown, then both. Watch for `0e8d:0003` (BROM) vs `0e8d:2000` (preloader
   USBDL).
2. If `0e8d:0003` appears, run a **read-only** command — `mtk printgpt` or
   `mtk r boot_para boot_para.bin`. Success means the fuse is not blown and
   everything changes. Handshake timeout means it is.

## Do not do these

- **Never write `preloader` or `lk`.** With BROM fused off there is no recovery
  path; mtkclient's own docs warn a wrong-header preloader write is terminal.
- **Never downgrade LK or firmware.** Anti-rollback is armed (`rpmb_state=2`);
  this is the classic Fire hard-brick.
- **Do not short eMMC test points.** On trona this variously did nothing,
  dropped to preloader, or prevented power-up entirely — and it cannot defeat
  the fuse anyway.

## What's left, ranked

| Path | Needs to be true | Risk | Odds |
| --- | --- | --- | --- |
| ~~`oem_unlock_supported=1` is real~~ | — | — | **tested, dead** |
| **Obtain & audit `yacht` LK** | a logic flaw in `amzn_*_unlock*`, or `dev_flags`/`fos_flags` opens a path | none (read/analyse only) | low, but the only route to a *permanent, untethered* unlock |
| BROM fuse not blown on this unit | built outside the fuse rollout | free to test read-only | very low; yields only a tethered unlock |
| Mali root → dump LK | 7.4.0.1 missed a backport | moderate | very low — see below |
| Forge a cert | break RSA-2048 | — | zero |
| Write IDME directly | — | — | zero (signature-checked) |

**Root via Mali GPU bugs** (CVE-2022-38181, GHSL-2023-005, CVE-2023-6241) does
cover MT8183 Fire tablets, but community consensus is everything is patched from
**Fire OS 7.3.2.9** up. This device is **7.4.0.1**, so they are almost certainly
dead — though `yacht` is an odd low-volume SKU and Amazon's backport discipline
on those is unknown, so confirming the Mali driver version is worth ten minutes.
Root wouldn't unlock anything, but it would let us `dd` the `lk` partition,
which unblocks the analysis path.

## The actual next move: get LK and reverse it

This is the only path whose ceiling is a real unlock, and it carries **zero
device risk** — it's host-side analysis.

Acquisition, cheapest first:

1. **A `yacht` OTA image**, if one can be fetched from Amazon's update servers —
   contains `lk`, needs no root and no BROM. This would collapse the whole
   problem into a Ghidra session. Search key is
   `com.amazon.pinnacles.android.os`.
2. Root → `dd if=/dev/block/mmcblk0p5`.
3. BROM readback, if the fuse test surprises us.

### Analysis workflow

Per [R0rt1z2's guide](https://blog.r0rt1z2.com/posts/reverse-engineering-mediatek-lk/),
the canonical write-up for exactly this:

1. **Extract.** `lkpatcher` parses the container and — importantly — prints each
   sub-partition's **load base address**:
   ```
   lkpatcher lk.bin --list-partitions
   lkpatcher lk.bin --dump-partition lk -o lk_raw.bin
   ```
   Use it only as a parser; we are not patching. LK v2.0 containers hold `lk`,
   `bl2_ext`, `aee`, `lk_main_dtb`, `lk_dtbo`, each with its own `cert1`/`cert2`.
   The LK code itself is **not compressed** — a raw image, which is what makes
   static analysis straightforward.
2. **Ghidra language: ARM v8 64-bit LE.** LK v2.0 is ARM64; only legacy v1.0 is
   ARMv7. Our 2022 BSP means v2.0 — get this wrong and nothing disassembles.
3. **Set the base address** to what lkpatcher printed; don't guess it. Sanity
   check: `ADRP`/`ADD` pairs should start resolving into the string region.
4. **Fix the memory map before auto-analysis** — mark RAM read+execute but
   **not write** (otherwise Ghidra treats globals as volatile and decompiler
   output degrades), and **disable "Eliminate Unreachable Code"** in the
   decompiler. That second one matters here specifically: we are hunting
   verification branches Ghidra might otherwise prune.

### The symbols are not exported — find them via strings

Amazon ships a stripped release build. The `amzn_*` names the community knows
are recoverable because **LK's logging macros embed `__func__` and function-name
literals in the string table** — someone simply ran `strings` on a dump.

So the method is **string → xref → containing function → rename**. Start with
`strings -a lk_raw.bin | grep -i amzn` before even opening Ghidra; that yields
the roster for *this* build in seconds and shows whether Amazon added or removed
routines since the trona-era analyses.

The behavioural strings are the real signposts:

- *"Device is temporarily unlocked, %d reboots remaining"* — xrefs straight into
  the **success branch** of the temp-unlock path. Walk backwards; the
  conditional guarding it is the check we care about.
- *"fail to pass RSA-PSS verification"* and the 256-byte PSS length checks —
  land in the crypto verifier.
- Also grep: `unlock`, `idme`, `t_unlock`, `dev_flags`, `fos_flags`,
  `boot_state`, `orange`, `green`.

MTK LK is a debug-heavy codebase and Amazon did not strip the format strings,
which makes this far more tractable than "reverse a stripped bootloader" sounds.

### Where the public key probably lives

Three hypotheses, in order of likelihood:

1. **Embedded in LK as a constant** (most likely) — find the 256-byte
   high-entropy modulus referenced by the verifier, or a `{n, e}` struct with
   `e = 0x10001`. Earlier Fire LKs did it this way. Mildly good news: an in-LK
   key means the check is self-contained and a logic flaw in it is exploitable
   without fighting the TEE.
2. **Read from `keys` (p3) at runtime** — would show as the key pointer tracing
   back to a partition read rather than a `.rodata` address.
3. **Delegated to TEE** — would appear as an SMC call instead of in-LK RSA.
   Unlikely, since the RSA-PSS strings are *in LK*.

On `kb` (p1), `dkb` (p2), `keys` (p3): the likeliest reading is **keybox /
device-keybox** material (Widevine, attestation) rather than the unlock anchor —
because the unlock verifier needs a key *identical across every `yacht`*, while
those look per-device provisioned, which is the wrong shape for a vendor signing
key. Unverified; confirm by tracing the key pointer rather than assuming.

### Ranked attack surface, once LK is in hand

| Rank | Surface | Why |
| --- | --- | --- |
| 1 | **Debug/eng path gated on a writable flag** (`dev_flags`, `fos_flags`) | **No cryptography needed.** IDME as a whole is *not* signature-protected — only the `unlock` blob is. If a consumer branch sits upstream of or bypasses the unlock check, and the HAL exposes a setter reachable with root, that's a complete chain. |
| 2 | **Length/parse bug in cert handling** | The visible 256-byte PSS length validation is exactly the check whose mismatch-with-actual-length has burned many vendors. Statically auditable. |
| 3 | **Unchecked return values around the verify call** | Classic `memcmp` truncation / error path falling through. Five-minute read of every call site once the verifier is located. |
| 4 | **Fastboot command-handler bugs in LK** | Reachable from the bootloader mode we already have — and the restriction check itself is code worth auditing. amonet carried an LK-stage exploit for Fire HD 8 2018, so Amazon LK bugs have precedent. Needs neither root nor BROM. |
| 5 | Patching LK to skip the check | Not independent — requires flashing LK, which requires beating the preloader, which requires BROM. Yields only a *tethered* unlock. |
| 6 | LK downgrade | Blocked twice (anti-rollback armed + preloader verifies LK). **Most likely way to hard-brick.** |
| 7 | Forge the signature | RSA-2048. Infeasible. |
| 8 | Write IDME directly | Zero value alone — only useful as the *delivery* mechanism for 1–3. |

Honest expectation: auditing a modern secure-boot-verified LK for a flaw a
motivated community hasn't found in four years on the sibling device. Low
probability — but free to try, zero device risk, and the payoff is the goal.

## Blocked pending root

These were the cheap tests worth running; all but one need privileges we don't
have. Recording so they get run the moment root lands:

- **Does `unlock_version` encode the device binding?** If `7ebd9a96` (or its
  byte-reversal) matches the eMMC PSN and `0c71a100` relates to `manfid`/CID,
  the device-binding claim upgrades from "inferred from 2014-era firmware" to
  confirmed — and we'd know the exact signed message, which is the input needed
  before reversing the verifier. Blocked:
  `/sys/block/mmcblk0/device/{manfid,serial,cid}` are all `Permission denied`.
- **Partition sizes for `kb`/`dkb`/`keys`** — KB-scale means key material,
  MB-scale means firmware. Blocked: `/proc/partitions` denied and
  `/sys/class/block/mmcblk0p*/size` unreadable.
- **The IDME HAL write path** — the service is
  `/vendor/bin/hw/fireos.hardware.idme@1.0-service`, running as **user
  system**. A HIDL HAL with a getter very often has a setter, and that decides
  whether attack-surface item 1 is reachable at all. Blocked: `/vendor/bin/hw`
  is not readable by shell.

The one unblocked lead is the **OTA hunt** below.

## Prior art: there is none

**No Amazon device released after ~March 2020 has been bootloader-unlocked by
anyone.** The most recent success is the Fire TV Stick 3 / Lite (`sheldon`,
2020), which needs Fire OS < 7.2.7.3 and is dead on shipping firmware.
`amonet`/`cuber` cover pre-2020 hardware only.

What would change the verdict: a public `yacht` OTA; a published DA-level or
preloader-level exploit for **V5/XFLASH** MT8183 (hwcode `0x788`) reachable from
preloader USBDL. On the latter — R0rt1z2's
[heapb8 DA exploit](https://blog.r0rt1z2.com/posts/exploiting-mediatek-datwo/)
is exactly the right shape, needing only Preloader USBDL and bypassing
DAA/SLA/SBC even where OEMs disabled BootROM USBDL — but it targets the **V6/XML
DA** protocol introduced around 2022, and MT8183 uses V5/XFLASH. It does not
apply today; an equivalent V5 DA bug would reopen everything. Worth watching.

## Sourcing caveat

`xdaforums.com` returns HTTP 403 to all automated fetching, so the XDA-derived
claims above (the e-fuse offset, the `mid`+`psn` unlock format) reached us only
through search-engine extracts and are **second-hand**. Those two in particular
are worth verifying in a real browser before acting on them. Threads to read
manually:

- [Fire HD 10 11th gen (trona) unlock brainstorming](https://xdaforums.com/t/fire-hd-10-11th-generation-2021-bootloader-unlock-root-brainstorming.4509197/) — closest relative
- [Fire HD 10 2019 bootless root / unlock brainstorming](https://xdaforums.com/t/new-fire-hd10-2019-bootless-root-method-bootloader-unlock-brainstorming.3979343/) — pp. 29–48 are the LK/IDME/efuse core
- [HD 8 2020 bootrom point location](https://xdaforums.com/t/hd-8-2020-bootrom-point-location.4427579/) — the efuse bit
- [Fire HD 10 13th gen (tungsten)](https://xdaforums.com/t/fire-hd-10-13th-generation-2023-tungsten-kftuwi-bootloader-unlock-root-brainstorming.4686124/) — current state of the art

Other sources: [bootloader-unlock-wall-of-shame (Amazon)](https://deepwiki.com/zenfyrdev/bootloader-unlock-wall-of-shame/2.10-amazon),
[R0rt1z2/amonet](https://github.com/R0rt1z2/amonet),
[bkerler/mtkclient](https://github.com/bkerler/mtkclient),
[AOSP locking/unlocking](https://source.android.com/docs/core/architecture/bootloader/locking_unlocking).

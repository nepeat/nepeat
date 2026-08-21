# Bootloader unlock — feasibility

**Current verdict: not feasible by any published method, and the most promising
lead has now been tested and failed.** This file records why, so nobody
re-treads it.

Research 2026-08-21 plus a live fastboot test on the device. Raw output in
[`dumps/fastboot-getvar.txt`](dumps/fastboot-getvar.txt).

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

## How the unlock actually works

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

Analysis, per [R0rt1z2's guide](https://blog.r0rt1z2.com/posts/reverse-engineering-mediatek-lk/):
extract with `lkpatcher` (emits the LK sub-partition and its load base), import
to Ghidra as ARMv8 for LK v2.0 (matching our 2022 BSP), set the base address,
disable RAM write perms, disable "Eliminate Unreachable Code" in the decompiler,
then analyse. LK v2.0 containers hold `lk`, `bl2_ext`, `aee`, `lk_main_dtb`,
`lk_dtbo`, each with `cert1`/`cert2`.

Targets once loaded: `amzn_unlock_verify`, `amzn_verify_temp_unlock_code`, the
three `amzn_get_temp_unlock_idme_*` accessors, the *"temporarily unlocked, %d
reboots remaining"* format string (xref back to its success branch), the
embedded RSA-2048 modulus, the consumers of `dev_flags` / `fos_flags`, and
whatever reads `unlock_version` (8 bytes, `7ebd9a960c71a100` here).

Also worth a hypothesis when LK is in hand: what the small `keys` (p3), `kb`
(p1) and `dkb` (p2) partitions hold, and whether they are the trust anchor.

Honest expectation: auditing a modern secure-boot-verified LK for a flaw a
motivated community hasn't found in four years on the sibling device. Low
probability — but free to try and the payoff is the whole goal.

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

# LK analysis — from a reference Amazon MT8183 bootloader

No firmware exists publicly for `yacht`/`pinnacles`. But a **contemporaneous
Amazon MT8183 LK** was obtained from a public `trona` (Fire HD 10 11th gen,
retail sibling) OTA, and it is close enough to work against.

| | |
| --- | --- |
| Build | Fire OS 7.3.2.6, `PS7326.3183N`, incremental `0025602846464` |
| **Our device** | Fire OS 7.4.0.1, `PS7401.3594N`, incremental **`0025535842816`** |
| Size / sha256 | 604,912 / `71d74575…1d063dd` |

The incrementals are **days apart** — about as close a contemporaneous Amazon
MTK LK as exists. Signing keys will differ, but layout, string roster and
routine positions should transfer almost directly. A second image
(`PS7331`, 617,232 bytes) is staged as a diff target.

Files are in `ref-firmware/` (gitignored — public firmware, but binary).
Retrieved without downloading the 1.3 GB OTAs: the images are plain zips, so the
agent parsed the EOCD/central directory over HTTP range requests and pulled only
the ~344 KB compressed `images/lk.img` member.

## Confirmed: this is UFBL, with LibTomCrypt RSA-PSS

Build paths left in the binary independently corroborate the leaked-source
account in [unlock.md](unlock.md):

```
/mnt/build/workspace/wsn1/bootable/bootloader/ufbl-features/project/../features/
    libtomcrypt/src/pk/rsa/rsa_verify_hash.c
    libtomcrypt/src/pk/pkcs1/pkcs_1_pss_decode.c
    libtomcrypt/src/pk/asn1/der/sequence/der_decode_subject_public_key_info.c
```

So: **UFBL + LibTomCrypt + RSA-PSS + X.509/DER**, verified from the binary
rather than taken on trust.

## The `amzn_*` routine roster

Complete, and it is the hunting list for Ghidra:

```
amzn_verify_unlock                  amzn_verify_temp_unlock_code
amzn_verify_code_internal           amzn_get_temp_unlock_idme_code
amzn_get_temp_unlock_idme_cert      amzn_get_temp_unlock_idme_data
amzn_get_temp_unlock_current_code   amzn_image_verify
amzn_lk_verify_image_maybe          amzn_check_keys_partition
amzn_fastboot_cmd_dump              amzn_set_mnt_keys_rw_opts
amzn_plat_alloc
```

`amzn_check_keys_partition` settles an open question: the **`keys` partition
(p3) is consumed by LK**, so it is a live candidate for where key material is
read from.

Supporting error strings give the control flow for free:

```
%s: Failed to get unlock code          %s: Failed to get unlock key
%s: Failed to get temp unlock cert     %s: Failed to get temp unlock codes
%s: Verify temp unlock cert fail, ret = %d
%s: idme_get_var_external fail
Device is temporarily unlocked, %d reboots remaining
Cannot find signature in user certificate    Failed to decode signature
Error, failed to read idme from boot area.
```

That last one independently confirms IDME lives in the eMMC **boot area**, not
the user partition.

## The find that matters: dev_flags and fos_flags

```
[SELINUX] set to permissive mode by dev_flags
[SELINUX] enforced by dev_flags
[DM-VERITY] verify off by fos_flags
[DM-VERITY] Found vendor fsmgr_flags value is [%s]
[DM-VERITY] change fsmgr_flags to [%s]
```

**`dev_flags` sets SELinux permissive. `fos_flags` turns off dm-verity.**

Those two IDME fields — both `0` on our device — are precisely the switches you
would need to run a modified system image. This reframes the goal: you may not
need a *bootloader unlock* at all, you need these two flags set. That is a much
smaller target than forging an RSA-2048 signature.

The `oem` command surface that sets them:

```
oem flags [<type>: <modifier>] <value>
oem idme            oem relock            oem dump-boot-args
oem logcat          oem off-mode-charge   oem reboot-recovery
oem reset-rtc-gauge
%s: Assuming fos_flags. Use <flag_type>:<flag_value> to use another.
%s: Managed to set flags.
```

### And the gate

```
Only usr_flags can be set for a locked device
```

So on a locked device only `usr_flags` is writable; `dev_flags` and `fos_flags`
are refused. **That string is the single highest-value target in the binary** —
find the check it guards and you learn whether the gate is a clean
`if (!unlocked)` or something with an exploitable edge.

Confirmed empirically on our hardware (2026-08-21): every `oem` command tested,
including the read-only `oem logcat lk` and `oem dump-boot-args`, returns
*"the command you input is restricted on locked hw"*. The locked-hw allowlist is
brutally small — only `getvar product`, `getvar serialno` and
`getvar max-download-size` answer. See
[`dumps/fastboot-oem-probe.txt`](dumps/fastboot-oem-probe.txt).

So the flags are unreachable *via fastboot*. The open question is whether
they're reachable another way — the IDME HAL at runtime with root (there is an
`oem idme` command, so a write path exists in principle), or via a flaw in the
gate itself.

## An engineering-device path exists

```
$Common Kernel Signing Engineering CA
#Common Kernel Signing Production CA
Authentication failed on engineering device with production certificate
Image FAILED AUTHENTICATION on ENGINEERING device
Image AUTHENTICATED with PRODUCTION certificate
Cannot get custom public key, abort in 5 seconds
[%s][%s] Only try verify %s with prod key on locked production device
```

LK carries **two signing CAs** and branches on whether the device is
engineering or production. Interesting because ours is a non-retail internal
SKU — but `ro.boot.prod=1` says this unit is on the production side, and the
last string says a locked production device only ever tries the prod key. Worth
understanding how that determination is made (fuse? IDME?), though it does not
look promising.

Anti-rollback is per-component, confirming the downgrade warnings:
`antirback_lk_version`, `antirback_pl_version`, `antirback_tee_version`,
`antirback_spm_version`, `antirback_sspm_version`, `antirback_vpu_version`.

## Next steps on the binary

1. **Load `trona_PS7326_lk.img` into Ghidra** per the workflow in
   [unlock.md](unlock.md) — unpack with `lkpatcher` for the load base, ARM v8
   64-bit, RAM read+execute only, disable "Eliminate Unreachable Code".
2. **Xref `"Only usr_flags can be set for a locked device"`** first. That is the
   gate on the two flags that actually matter.
3. Then `amzn_verify_temp_unlock_code` and `amzn_verify_unlock`, hunting a
   length/parse bug in `idme_get_var_external` or an ignored PSS return.
4. Locate the embedded RSA-2048 modulus via xrefs from the verifier — and check
   whether it is a `.rodata` constant or read via `amzn_check_keys_partition`.
5. Diff `PS7326` against `PS7331` to see what Amazon changed between builds;
   divergence often marks security fixes.

Ghidra plus a Ghidra MCP server are available in this environment, so this can
be driven directly rather than by hand.

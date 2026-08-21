# Hardware & platform — from the device

Everything here came out of an authorized `adb` shell on 2026-08-20. Raw
artifacts are in [`dumps/`](dumps/). This supersedes the photo-derived guesses
in the original handoff.

Shell is unprivileged, so `/proc/cmdline`, `/proc/partitions`,
`/proc/bus/input/devices` and `/system/build.prop` all returned
`Permission denied`. Everything below came from `getprop`, `dumpsys`, and
world-readable `/proc` entries.

## SoC and platform

| | |
| --- | --- |
| SoC | **MediaTek MT8183** (`ro.hardware`, `ro.board.platform`, `/proc/cpuinfo` `Hardware: MT8183`) |
| Chip rev | `S01` (`ro.vendor.mediatek.chip_ver`) |
| CPU | 8× ARMv8, CPU part `0xd09` = Cortex-A73, rev 2 |
| MTK BSP | `alps-mp-p0.mp1.tc6sp-of.p12` |
| RAM | 3 895 380 kB ≈ **4 GB** |
| Storage | 32 GB eMMC — 24 GB `/data`, 3.4 GB system, 519 MB cache |
| Display | **1200×1920**, density 240 |
| Battery | `CHARGE_FULL` 6 500 000 µAh = **6500 mAh**, Li-ion |
| Wi-Fi | `CONSYS_MT8183` (MediaTek integrated) |
| NFC | **NXP** (`ro.hardware.nfc_nci`), HAL running, `ro.vendor.nfc.support=1` |
| Audio | Dolby DAX (`com.dolby.daxservice`, `feature:amazon.dolby_dax`) |

The handoff's MediaTek guess from kernel 4.4.146 was **correct**, and it's now
confirmed three independent ways rather than inferred.

**Userspace is 32-bit only** on this 64-bit SoC — `ro.product.cpu.abi` is
`armeabi-v7a` and `ro.product.cpu.abilist64` is *empty*. Notable both as a
platform oddity and practically: anything built to run here must be 32-bit ARM.

## Cameras

Two, from `dumpsys media.camera`:

- **Rear — `Has a flash unit: true`**
- Front — no flash

Capability flags include `camera.level.full`, `manual_sensor`,
`manual_post_processing` and `raw`.

This settles the XDA rear-flash claim: **it's real.** (The 4 GB figure from the
same post also checks out. Both were guesswork by that poster, but both happen
to be right.)

## No modem

`android.hardware.telephony` is **absent** from `pm list features`, and
`dumpsys telephony.registry` shows `mRilVoiceRadioTechnology=0(Unknown)`,
`mCellInfo=null`, everything `OUT_OF_SERVICE`.

So the open question resolves as: **the telephony *framework* is present but
there is no modem hardware.** The AOSP telephony packages
(`com.android.phone`, `providers.telephony`, `mms.service`, `carrierconfig`,
`simappdialog`, `smspush`) are all installed, which is what renders "No SIM
card — No service" on the lockscreen and populates the SIM/IMEI rows in
Settings. They're stock AOSP baggage with no RIL underneath, not evidence of a
radio.

## Partition layout

Textbook MediaTek, single-slot (no A/B), all on `mmcblk0`:

```
kb p1   dkb p2   keys p3   misc p4   lk p5   tee1 p6   tee2 p7
metadata p8   boot_para p9   nvcfg p10   spmfw p11   sspm_1 p12
cam_vpu1 p13  cam_vpu2 p14  cam_vpu3 p15
boot p16   recovery p17   cache p18   system p19   vendor p20   userdata p21
```

`cam_vpu1..3` are dedicated camera-VPU firmware partitions — consistent with a
device where imaging matters.

## Boot chain and lock state

This is the part that matters for custom firmware:

```
ro.boot.flash.locked      1          <-- bootloader LOCKED
ro.oem_unlock_supported   1          <-- but unlocking IS supported
ro.boot.unlocked_kernel   false
ro.boot.verifiedbootstate green
ro.boot.veritymode        eio
ro.boot.selinux           enforcing
ro.boot.secure_cpu        1
ro.secure / ro.adb.secure 1
ro.boot.prod              1
ro.boot.atm               disabled   (Android Test Mode off)
ro.boot.rpmb_state        2
```

**`ro.oem_unlock_supported=1` is the headline.** Retail Fire tablets ship this
disabled and hide the bootloader entirely — which also explains why the
handoff found "reboot to bootloader" exposed in recovery when retail units hide
it. The device is locked *now*, but the platform advertises that unlocking is a
supported operation. The next step is checking whether the OEM unlocking toggle
in developer options is present and settable, then `fastboot flashing
get_unlock_ability`.

Bootloader components predate the system image by over a year:

- preloader `549058d-20220401_090835` — 2022-04-01
- LK `e6b8902-20220815_072553` — 2022-08-15
- boot/system image — 2023-12-05

So hardware bring-up happened in H1 2022 and the shipping image was cut ~19
months later against a frozen 2022-01-01 security patch.

## Provisioning state — the enterprise-MDM theory is dead

`dumpsys device_policy`:

```
Enabled Device Admins (User 0, provisioningState: 0):
    <none>
mPasswordOwner=-1
Encryption Status: inactive
```

`dumpsys account` → **0 accounts**. `pm list users` → a single
`UserInfo{0:Owner:13}`. `pm list packages -3` → **zero third-party packages.**

There is no device owner, no profile owner, no MDM enrollment and no account of
any kind. Whatever this device did, it is not currently enrolled in anything,
and nothing was ever installed on top of the system image.

`sys.boot.reason` is **`reboot,factory_reset`** — the unit *was* factory reset.
That, not "never provisioned", is what explains the RTC falling back to the
kernel build date. The handoff correctly flagged this as an alternative
explanation; it's now the confirmed one.

Practical consequence: the "don't wipe, you'll hit an enrollment screen" worry
is moot. It has already been wiped, and it came up clean.

## No screen lock credential

From `logcat`:

```
LockSettingsStorage: Cannot read file java.io.FileNotFoundException:
  /data/system/gatekeeper.password.key: ... ENOENT
LockSettingsStorage: Cannot read file java.io.FileNotFoundException:
  /data/system/password.key: ... ENOENT
```

Neither credential file exists, so the padlock the handoff saw is a **swipe
lock, not a PIN or password**. There was never a credential to be locked out
by. Also of note: `Device doesn't implement AuthSecret HAL` and `Device does
not support weaver`, and `Encryption Status: inactive`.

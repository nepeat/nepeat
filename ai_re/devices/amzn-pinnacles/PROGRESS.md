# amzn-pinnacles — Amazon "yacht" / KFYAWI

**Identified 2026-08-20: a Fire HD 10 Plus (11th gen) hardware derivative with
NFC and a rear camera flash added, running an AOSP-app-layer Fire OS 7.4.0.1
build on MediaTek MT8183.** Not a retail product; never sold. No marketing name
found in any public source.

**Status: full ADB shell (unprivileged). Bootloader is locked, but
`ro.oem_unlock_supported=1` — an unlock path is advertised and untested.**
That's the next move.

Detail lives in siblings:

- **[raft-lockscreen.md](raft-lockscreen.md)** — the Kerberos shift-login
  keyguard, and why setting a PIN sends you to a username/password screen.
- **[hardware.md](hardware.md)** — SoC, partitions, boot chain, lock state, all
  from the device.
- **[identification.md](identification.md)** — how it was identified, plus the
  public-source research and what was ruled out.
- **[dumps/](dumps/)** — raw artifacts. **[apks/](apks/)** — pulled system APKs.

> ⚠️ **Careful with screen locks.** The keyguard is Amazon's RAFT corporate
> login. An alphanumeric password maps to a Kerberos account login that
> **cannot succeed on this device** (the authenticator is missing), and a
> numeric PIN maps to a "session PIN" validated against a session that does not
> exist. Pattern/swipe/none fall through to stock AOSP and are safe.
>
> **If you do get stranded:** `adb shell locksettings clear --old <credential>`
> — tested working, since ADB authorization survives the keyguard and
> `locksettings` bypasses RAFT entirely. Full detail and fallbacks in
> [raft-lockscreen.md](raft-lockscreen.md).

## Identity

| Field | Value |
| --- | --- |
| Build fingerprint | `Amazon/yacht/pinnacles:9/PS7401.3594N/0025535842816:user/amz-p,release-keys` |
| Fire OS | **7.4.0.1** (`PS7401/3594`) |
| `ro.product.model` | `KFYAWI` |
| `ro.product.name` / `device` / `board` | `yacht` / `pinnacles` / `pinnacles` |
| OTA package name | `com.amazon.pinnacles.android.os` |
| SKU | `ro.boot.hardware.sku` = `plus` |
| Characteristics | `tablet` |
| SoC | MediaTek **MT8183**, 8× Cortex-A73/A53 |
| RAM / storage | 4 GB / 32 GB eMMC |
| Display | 1200×1920, density 240 (~10.1") |
| Battery | 6500 mAh |
| Serial / DSN | `G002G402413303RL` |
| ASIN (rear label) | `B0BPK28DW2` — no Keepa history on .com or .co.uk |
| Wi-Fi MAC | `EC:A1:38:3E:C6:7D` |
| Android | 9 (API 28), patch 2022-01-01, kernel 4.4.146+ (2023-12-05) |
| Userspace | **32-bit only** — `abilist64` is empty |

## What the device settled

Four open questions from the handoff now have answers.

**There is no modem.** `android.hardware.telephony` is absent and the telephony
registry has no RIL. The AOSP telephony packages are installed, which is what
draws "No SIM card — No service" on the lockscreen and the SIM/IMEI rows in
Settings — stock AOSP baggage, not a radio.

**It was never enterprise-enrolled.** No device owner, no profile owner,
`provisioningState: 0`, zero accounts, zero third-party packages. The MDM
theory is dead.

**It was factory reset.** `sys.boot.reason` = `reboot,factory_reset`. That, not
"never provisioned", is why the RTC fell back to the kernel build date — the
alternative the handoff flagged turns out to be the right one. So the "don't
wipe or you'll hit an enrollment screen" concern is moot; it's already wiped
and came up clean.

**The MediaTek guess was right,** now confirmed three ways rather than inferred
from the kernel version.

Two XDA claims the handoff (and I) had downgraded to hearsay turn out to be
**correct**: 4 GB RAM and a rear camera flash. The poster was guessing, but
guessed right.

## Next step — bootloader unlock

`ro.boot.flash.locked=1` but `ro.oem_unlock_supported=1`. Retail Fire tablets
ship that second one disabled and hide the bootloader; this platform advertises
unlock as supported, which also explains why recovery exposes "reboot to
bootloader".

1. Settings → Developer options → is there an **OEM unlocking** toggle, and
   does it set?
2. Then, from the bootloader:

   ```bash
   fastboot -i 0x1949 getvar all 2>&1 | tee dumps/fastboot-getvar.txt
   fastboot -i 0x1949 flashing get_unlock_ability
   ```

   `getvar all` is still worth capturing regardless — it's the one surface not
   yet touched.

Unlocking wipes `/data`, which is empty anyway, so there's nothing to lose.

## Also worth doing

- **Pull the system image.** Shell is unprivileged so `/system/build.prop` and
  `/proc/cmdline` are unreadable. Unlock first, or pull partitions from
  fastboot, then `jadx` the eight Amazon APKs — `com.fireos.arcus.proxy` and
  `com.amazon.shpm` are the unfamiliar ones and may say what the device was for.
- **Search firmware archives for `com.amazon.pinnacles.android.os`.** That's
  the correct OTA package string; FTVDB 404s on it, but other archives and OTA
  endpoints may not.
- **Teardown** to confirm the NFC controller part and look for anything else
  added over the retail Fire HD 10 Plus board.

## Cautions

- **Do not accept OTAs.** Firmware is frozen at a Jan 2022 patch baseline; an
  update could close the unlock path. This is the one caution from the handoff
  that still fully applies.
- The DSN is Amazon's registration/blacklist identifier — keep it out of public
  posts.
- The "don't factory reset" caution is now retired: it has already been reset.

## Log (newest first)

- **2026-08-20**: Explained the username/password lockscreen — see
  [raft-lockscreen.md](raft-lockscreen.md). SystemUI is Amazon's
  **RaftSystemUI** (keeping the `com.android.systemui` name), which replaces
  the AOSP keyguard with a Kerberos corporate shift login: username/password →
  session PIN → unlocked, with a logout button. Decompiled
  `RaftKeyguardSecurityModel` — numeric quality maps to `SecurityMode.Session`,
  alphanumeric to `SecurityMode.Account`. The `raft_kerberos` binder service is
  registered, but the `com.amazon.kerberos` AccountAuthenticator is absent, so
  login can never succeed. Also found `com.amazon.redstone` with its APK
  stripped but native voice/gesture libs intact.
- **2026-08-20**: ADB authorized; ran `collect.sh`. **Device identified** — see
  [identification.md](identification.md). MT8183 / 4 GB / 1200×1920 / 6500 mAh
  / `sku=plus` matches Fire HD 10 Plus (11th gen) on every axis, but this unit
  adds NFC (NXP + Mifare) and a rear flash, which retail lacks, and replaces
  the Fire consumer shell with AOSP reference apps while keeping the Fire OS
  platform SDK. Confirmed no modem, no MDM enrollment, prior factory reset.
  Found `ro.oem_unlock_supported=1` against a locked bootloader. Wrote
  [hardware.md](hardware.md).
- **2026-08-20**: Public-source research pass. Decoded the build band
  (`PS7401` = Fire OS 7.4.0.1); the Echo Show 15 lead it produced was later
  refuted by the package list. Ruled out codename wikis, FTVDB, Amazon GPL
  source pages, Keepa (ASIN never retailed), and the XDA thread.
- **2026-08-20**: Created project. Added Android tooling to the RE flake
  (`android-tools`, `scrcpy`, `jadx`, `apktool`, `simg2img`,
  `payload-dumper-go`; `abootimg` skipped — Linux-only, use `binwalk` for
  boot.img on darwin). Confirmed the device on USB as `0x1949:0x0644` and read
  the descriptor to establish that ADB was already enabled.

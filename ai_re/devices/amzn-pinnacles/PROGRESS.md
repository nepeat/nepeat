# amzn-pinnacles — Amazon "yacht" / KFYAWI

**Identified 2026-08-20: a Fire HD 10 Plus (11th gen) hardware derivative with
NFC and a rear camera flash added, running an AOSP-app-layer Fire OS 7.4.0.1
build on MediaTek MT8183.** Not a retail product; never sold. No marketing name
found in any public source.

**Status: full ADB shell (unprivileged). Bootloader is locked, but
`ro.oem_unlock_supported=1` — an unlock path is advertised and untested.**
That's the next move.

Detail lives in siblings:

- **[customization.md](customization.md)** — what Amazon actually changed:
  boot-classpath framework, custom SELinux class, `fireos.hardware.*` HALs, and
  the IDME factory block (incl. the empty unlock fields).
- **[raft-lockscreen.md](raft-lockscreen.md)** — the Kerberos shift-login
  keyguard, why setting a PIN sends you to a username/password screen, and the
  hardcoded emergency credential.
- **[hardware.md](hardware.md)** — SoC, partitions, boot chain, lock state, all
  from the device.
- **[identification.md](identification.md)** — how it was identified, plus the
  public-source research and what was ruled out.
- **[dumps/](dumps/)** — raw artifacts. **[apks/](apks/)** — pulled system APKs.

> ⚠️ **Setting any screen lock hands you to Amazon's RAFT corporate login** —
> PIN, password *and* pattern alike, because RAFT discards the quality you chose
> and substitutes an uninitialised `enterprise_password_type` that defaults to
> "account login". Only leaving it unset keeps stock AOSP behaviour.
>
> **Emergency credential (hardcoded in the APK): blank username + password
> `letmein`.** Bypasses Kerberos entirely, then suppresses the ticket-expiry and
> clock-skew checks. Enrol a session PIN when prompted (typed twice) and the
> lockscreen behaves normally afterwards.
>
> **Or over ADB:** `adb shell locksettings clear --old <credential>` — tested
> working; ADB authorization survives the keyguard. Full detail in
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

**Strategy — the three goals converge on one capability.** Rooting, dumping,
and producing an unlock code all bottom out in the same prerequisite: getting
code/read access below the OS. Concretely, dumping **LK** (`mmcblk0p5`) turns
the unlock problem from *"forge an Amazon signature"* into *"read the
verification routine and find its weakness"* — LK is where the unlock check
lives, including the `amzn_get_temp_unlock_idme_*` accessors. Dumping the
**BootROM** (mtkclient `dumpbrom`) and the **preloader** (eMMC boot0/boot1,
outside the by-name table) establishes which stage actually enforces what.
So the dump work is not a parallel goal to the unlock work — it is the
precondition for it. Everything gates on whether MediaTek BROM/download mode is
reachable on this 2022 unit, or fused off.

**What the lock actually hangs on:** the IDME factory block exposes
`t_unlock_code` and `t_unlock_cert`, and both are **empty** on this unit — see
[customization.md](customization.md). That is the concrete reason
`flash.locked=1`. On Fire hardware the bootloader consults those fields, and
unlocking means getting a valid entry written there, historically an
Amazon-signed cert bound to the device serial. `/proc/idme/*` is read-only via
procfs; writes go through the vendor-side `fireos.hardware.idme@1.0::IIdme`
HAL. So `oem_unlock_supported=1` may mean "this platform has the mechanism",
not "you can turn it on from Settings" — verify before assuming.

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

- **2026-08-21**: Surveyed what Amazon actually customized — see
  [customization.md](customization.md). `fosframework.jar` is on the
  BOOTCLASSPATH and AOT-compiled into the boot image; `fosinit`/`fosservices`
  run inside `system_server`; 17 of 175 binder services are Amazon's; there is a
  custom SELinux **object class** `amazon_policies` with 12 permissions; and a
  `fireos.hardware.*` HIDL namespace. Found the **IDME factory block** at
  `/proc/idme` (26 fields) — `device_type_id=AJDZ5ML3MICE5`,
  `ro.build.lab126.project=yacht_fireos_ship_7401`, and **`t_unlock_code` /
  `t_unlock_cert` both empty**, which is what the bootloader lock rests on.
- **2026-08-21**: Found the **hardcoded emergency credential** in
  `RaftLockPatternUtils.verifyAccount()` — blank username + password `letmein`,
  checked *before* Kerberos, which then stores the username `backdoor` to
  suppress ticket-expiry and clock-skew checks. Also **corrected yesterday's
  analysis**: the redirect is not "PIN→Session", it is
  `RaftLockPatternUtils.getActivePasswordQuality()` discarding the real quality
  and substituting `lockscreen.enterprise_password_type` (default `COMPLEX` →
  Account) whenever any credential is set. So pattern is *not* a safe lock type,
  contrary to what was written yesterday.
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

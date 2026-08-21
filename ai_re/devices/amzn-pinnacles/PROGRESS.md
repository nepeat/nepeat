# amzn-pinnacles — Amazon "yacht" / KFYAWI

**IDENTIFIED: an Amazon employee work tablet.** `yacht` / KFYAWI is a
non-retail, Amazon-internal variant of the **Fire HD 10 11th gen (2021)**,
issued to staff in **Europe** as a work device — MediaTek MT8183, 4 GB, with NFC
and a rear camera flash added over retail, running an AOSP-app-layer Fire OS
7.4.0.1 build whose lockscreen is a **Kerberos corporate shift login**. Retail
sibling is `trona` / KFTRWI. Never sold; no marketing name because it was never
marketed. See [identification.md](identification.md).

**✅ ROOTED 2026-08-21** — `uid=0(root) context=u:r:kernel:s0`, SELinux
**Permissive**, via CVE-2022-38181 (unpatched on this build). All key partitions
dumped including **`lk`**. See [root.md](root.md).

**Custom-ROM outlook: root is per-boot; a bootloader unlock is still not
available.**
Amazon's LK contains no unlock commands at all, and the only unlock surface
needs an RSA-2048 Amazon signature nobody has ever obtained. So the realistic
ceiling is per-boot root, permissive SELinux and debloat — not LineageOS. See
[root.md](root.md) and [unlock.md](unlock.md).

Detail lives in siblings:

- **[lk-reversing.md](lk-reversing.md)** — **our own LK reversed.** The unlock
  key is unforgeable (RSA-2048, keys extracted), but `dev_flags`/`fos_flags` in
  the *unsigned* IDME structure are writable with root and give SELinux
  permissive + dm-verity off without any unlock. **Decision pending.**
- **[lk-analysis.md](lk-analysis.md)** — analysis of a contemporaneous Amazon
  MT8183 bootloader. **`dev_flags` sets SELinux permissive and `fos_flags`
  turns off dm-verity** — which may matter more than a bootloader unlock.
- **[root.md](root.md)** — **root achieved**, how to re-obtain it, and the
  firmware dump inventory.
- **[unlock.md](unlock.md)** — bootloader unlock feasibility. Short version:
  **dead**. `oem_unlock_supported=1` tested and disproved; LK has no unlock
  commands; only path with any ceiling is reversing LK.
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

## Next step

`lk` is now dumped from this device, so the analysis can start. It carries the
same `amzn_*` roster and the same `dev_flags`/`fos_flags` strings as the trona
reference, so [lk-analysis.md](lk-analysis.md) transfers directly.

1. **Load `fw-partitions/dump/lk.img` into Ghidra** and xref
   `"Only usr_flags can be set for a locked device"` — that gate stands between
   us and setting `dev_flags` (SELinux permissive at boot) and `fos_flags`
   (dm-verity off), which together matter more than a bootloader unlock.
2. **Check whether root can reach the flags via the IDME HAL** —
   `/vendor/bin/hw/fireos.hardware.idme@1.0-service` is now readable, and LK has
   an `oem idme` command, so a write path exists in principle.
3. Extract the kernel from `boot.img` and run `vmlinux-to-elf` for kallsyms —
   this would give `selinux_enforcing` and make the exploit's own SELinux stage
   work rather than relying on the modprobe side-effect.
4. Reverse `amzn_verify_temp_unlock_code` hunting a length/parse bug in
   `idme_get_var_external` or an ignored PSS return.

Still unresolved and cheap: the BROM fuse test (read-only USB probe).

## Also worth doing

- **`jadx` the Amazon APKs** now pulled into `fw/priv-app` —
  `com.fireos.arcus.proxy` and `com.amazon.shpm` are the unfamiliar ones and may
  say more about what the device did.
- **Deoptimise `boot-fosframework.oat/.vdex`** to get readable code for Amazon's
  framework; the `.jar` files are stubs.
- **Search firmware archives for `com.amazon.pinnacles.android.os`.** That's
  the correct OTA package string; FTVDB 404s on it, but other archives and OTA
  endpoints may not.
- **Teardown** to confirm the NFC controller part and look for anything else
  added over the retail Fire HD 10 Plus board.

## Cautions

- **Do not accept OTAs.** An update would patch CVE-2022-38181 and cost us
  root. Amazon fixed it in 7.3.2.9 (June 2024); we are on an older internal
  train. This is the one caution from the handoff
  that still fully applies.
- The DSN is Amazon's registration/blacklist identifier — keep it out of public
  posts.
- The "don't factory reset" caution is now retired: it has already been reset.

## Log (newest first)

- **2026-08-21**: Reversed **our own** `lk.img` — see
  [lk-reversing.md](lk-reversing.md). **Unlock key generation is definitively
  impossible**: confirmed the signed message format `0x%08x%08x%08x`
  (SoC_ID, HW_ID, unlock_version) at `0x4b9fa` and extracted **six embedded
  RSA-2048 public keys**, two of which sit in the unlock code. Found a second
  fastboot entry point, `flash:tucert`. **But** found a better path: LK
  validates IDME by **magic number only** — no CRC or signature over the field
  table — and `dev_flags`/`fos_flags` are plain ASCII at byte offsets `0x2290`
  and `0x22b4` of `mmcblk0boot1`, with `force_ro` clearable by root. The
  *"Only usr_flags…"* gate is in the **fastboot** handler, which root bypasses.
  Two single-byte writes would give SELinux permissive + dm-verity off on a
  locked bootloader. Not attempted — needs a decision.

- **2026-08-21**: **ROOT.** CVE-2022-38181 confirmed unpatched (`jit_trigger`),
  then `exploit_trona` succeeded — `uid=0(root) context=u:r:kernel:s0`, SELinux
  **Permissive**. The exploit *reported failure*: its win-check reads
  `/data/local/tmp/pwned` while the payload writes `pwned2`, so the chain had
  already won while the tool said it hadn't. Also corrected an earlier wrong
  call of mine — aarch64 static binaries **do** run here (arm64 kernel, 32-bit
  Android userspace only). Dumped `lk`, preloader (boot0), IDME (boot1), boot,
  recovery, tee1/tee2, keys, kb, dkb, misc, boot_para, nvcfg, gpt — plus 1.3 GB
  of readable `/system`+`/vendor` without root. Our own `lk.img` carries the
  identical `amzn_*` roster and dev_flags/fos_flags strings as the trona
  reference, so that analysis transfers. Secrets (`keys`/`kb`/`dkb`/IDME)
  gitignored; hashes committed.

- **2026-08-21**: Obtained a **contemporaneous Amazon MT8183 LK** — no `yacht`
  firmware exists publicly, but a `trona` (retail sibling) OTA yielded an
  `lk.img` from build `PS7326`, incremental **days apart** from ours. Confirmed
  from the binary that this is UFBL + LibTomCrypt RSA-PSS, recovered the full
  `amzn_*` routine roster, and found that **`dev_flags` sets SELinux permissive
  and `fos_flags` disables dm-verity** — the two switches actually needed to run
  a modified system, and a far smaller target than forging RSA-2048. Gate is
  *"Only usr_flags can be set for a locked device"*, now the top Ghidra target.
  Confirmed on hardware that **all** `oem` commands are refused while locked,
  including read-only `oem logcat`/`oem dump-boot-args`. See
  [lk-analysis.md](lk-analysis.md).
- **2026-08-21**: **DEVICE IDENTIFIED** — an Amazon **employee work tablet**, a
  non-retail internal variant of the Fire HD 10 11th gen issued to staff in
  Europe, identified via the XDA Fire Toolbox community from an employee's own
  unit. This corroborates the hardware analysis *and* explains the RAFT Kerberos
  shift login. Also mapped the root path — see [root.md](root.md): `/dev/mali0`
  is world-accessible (`crw-rw-rw-`), `PS7401` qualifies for the free
  CVE-2024-31317 system-UID exploit (< `PS7704`), and there is a published
  CVE-2022-38181 exploit for this exact SoC/kernel/ABI whose Amazon fix
  (7.3.2.9, June 2024) postdates this build by six months. `adb root` refused —
  `ro.debuggable=0`, production build. Unlock confirmed dead: R0rt1z2's Ghidra
  work on stock Amazon LK found *no reference* to `flashing unlock`/`oem unlock`
  at all.
- **2026-08-21**: **Tested the unlock lead on hardware — it's dead.** Rebooted
  to fastboot: `getvar all`, `oem device-info`, `flashing get_unlock_ability`
  and `oem lks` all return *"the command you input is restricted on locked hw"*.
  Only `product`, `serialno` and `max-download-size` answer, so Amazon's LK runs
  a command allowlist while locked, and `ro.oem_unlock_supported=1` is an
  unscrubbed build flag rather than a policy difference. Research also resolved
  the mechanism: unlock is an RSA-2048 signature verified in LK over a
  device-bound blob, `rpmb_state=2` is **anti-rollback and not** unlock state
  (so the RPMB worry is retired), and BROM is fused off on 2020+ Amazon
  hardware. Writing IDME is worthless without Amazon's key. Only live path is
  dumping and reversing LK — see [unlock.md](unlock.md). Device returned to
  Android with ADB intact.
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

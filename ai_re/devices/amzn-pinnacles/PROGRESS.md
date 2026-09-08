# amzn-pinnacles — Amazon "yacht" / KFYAWI

**IDENTIFIED: an Amazon employee work tablet.** `yacht` / KFYAWI is a
non-retail, Amazon-internal variant of the **Fire HD 10 11th gen (2021)**,
issued to staff in **Europe** as a work device — MediaTek MT8183, 4 GB, with NFC
and a rear camera flash added over retail, running an AOSP-app-layer Fire OS
7.4.0.1 build whose lockscreen is a **Kerberos corporate shift login**. Retail
sibling is `trona` / KFTRWI. Never sold; no marketing name because it was never
marketed. See [identification.md](identification.md).

**🔑 UNLOCK MECHANISM FULLY MAPPED 2026-08-21.** The whole temp-unlock scheme
is now reconstructed and verified. Two new unauthenticated write primitives were
found, and the central "could a leaked credential unlock the family?" question
is answered — **no**.

* **`fastboot flash tucode` exists**, alongside `tucert`. The locked-hw
  allowlist is exactly `{oem relock, oem flags, flash:unlock, flash:tucert,
  flash:tucode}` (verified by reading the table at `0x8366c`, base
  `0x56000000`). So **both halves of a temp-unlock credential are installable on
  a locked, unrooted device.**
* **But there is nothing durable to install.** The ten 32-byte codes are
  `HMAC-SHA256(S_device, counter)` where `S_device` is 32 RNG bytes sealed in
  **eMMC RPMB block 1** and the counter increments **every boot**. A cert+code
  pair cannot unlock another unit (2⁻²⁵⁶) and expires within 10 reboots. Amazon
  temp unlock is an online challenge/response. See
  [unlock-codes-rpmb.md](unlock-codes-rpmb.md).
* **`fos_flags`/`dev_flags` (dm-verity off / SELinux permissive) are a decoy** —
  the restriction gate sits *inside* the flag-test primitive, so even a
  successful write returns 0 on a restricted device. See
  [fos-flags.md](fos-flags.md).
* **One live primitive remains:** the USBDL **PMIC** commands (`0xC6`/`0xC7`)
  never call `sec_region_check` and so bypass the memory whitelist entirely.
  Orthogonal to SBC/DAA/BROM-fused. Read-only probes are safe; rail writes are
  potentially terminal. See [usbdl-pmic-primitive.md](usbdl-pmic-primitive.md).

**⛔ RETRACTED 2026-08-21 — there is no root-free preloader memory write.** An
earlier entry here claimed one was confirmed. It was not: that claim came from
misreading mtkclient log lines which describe **host-side** patching of the DA
blob, not writes to the device. Tested live, the preloader's USBDL memory
commands are **address-filtered to a 5-address allowlist** (the eFuse window
plus the WDT); `WRITE16` to the patch target `0x0022D8B8` returns status
`0x1001` (denied), and even `READ32` there returns `0x1000`. See
[preloader-usbdl.md](preloader-usbdl.md).

What *is* real from that work: preloader USBDL is reachable purely from software
with no buttons (`adb reboot`), the handshake succeeds on a locked unrooted
device reporting `HW code: 0x788`, and **`efuse 0x11f10060 = 0x00000946` was
read live off the device** — independently confirming the value previously
derived from `atag,devinfo`.

**✅ ROOTED 2026-08-21** — `uid=0(root) context=u:r:kernel:s0`, SELinux
**Permissive**, via CVE-2022-38181 (unpatched on this build). All key partitions
dumped including **`lk`**. See [root.md](root.md).

**Custom-ROM outlook: root is per-boot; a bootloader unlock is still not
available, but custom kernel code now runs.**
Amazon's LK contains no unlock commands at all, and the only unlock surface
needs an RSA-2048 Amazon signature nobody has ever obtained. Directly flashing
an unsigned boot/recovery remains blocked, but the verified unsigned-module
path makes a stock-signed boot followed by module-assisted second-stage or
warm boot technically credible. See [root.md](root.md),
[source-kernel.md](source-kernel.md), and [unlock.md](unlock.md).

**⭐ NEW 2026-08-25 — exact 4.4.146 kernel source recovered.** Amazon's
official Fire HD 10 11th-gen GPL archive contains the exact kernel version,
MT8183 tree, `trona_defconfig`, and build scripts. The live yacht config passes
`olddefconfig`/`prepare`/`modules_prepare`, removing the 4.4.302 source mismatch
that blocked unsigned-module work. Also corrected a false lead:
`masp_hal_set_dm_verity_error()` is a source-confirmed no-op dummy. The better
code-signing bypass is now a stock-signed first boot followed by either a
module-assisted second-stage userspace or, later, a module-carried arm64 warm
boot. See [source-kernel.md](source-kernel.md).

Detail lives in siblings:

- **[unlock-scheme.md](unlock-scheme.md)** — 🔑 **the temp-unlock mechanism,
  reconstructed.** cert + codes + signature, both phases, and why our
  unauthenticated write cannot be levered.
- **[unlock-codes-rpmb.md](unlock-codes-rpmb.md)** — 🔑 **the codes are an RPMB
  per-boot nonce.** Credentials are neither portable nor durable. Also: ARB is
  RPMB-backed, koboreru marker absent, and the scene survey.
- **[fos-flags.md](fos-flags.md)** — ⛔ **dm-verity/SELinux switches are a
  decoy**, gated inside the test primitive. Includes the verified locked-hw
  allowlist and three corrections to older notes.
- **[usbdl-pmic-primitive.md](usbdl-pmic-primitive.md)** — ⭐ **the one surviving
  USBDL primitive**: PMIC commands bypass the memory whitelist. Read probes safe,
  rail writes potentially terminal.
- **[preloader-usbdl.md](preloader-usbdl.md)** — ⛔ **USBDL memory access is
  address-filtered; the patch is denied.** Retracts the earlier "root-free
  arbitrary memory write" claim. Keeps what is real: software-only preloader
  entry, a live handshake on a locked device, the measured allowlist, the
  protocol framing, and the macOS serial-path requirement.
- **[usbdl-memory-commands.md](usbdl-memory-commands.md)** — ⛔ **closed.** The
  preloader's USBDL handler does implement `READ32`/`WRITE16`, but they are
  restricted to a 5-address allowlist. Note this file's opcode table is
  superseded by [preloader-usbdl.md](preloader-usbdl.md). Read-only test
  defined, zero risk.
- **[efuse-answer.md](efuse-answer.md)** — ⭐ **the eFuse question is ANSWERED**:
  `0x11f10060 = 0x946` → SBC **enabled** (DA validation enforced) and BROM
  download **fused off**. Read without UART, from data already dumped.
- **[theories-closed.md](theories-closed.md)** — everything **tested and ruled
  out**, so it isn't retried. Read before proposing a new angle.
- **[da-validation.md](da-validation.md)** — ⛔ **CLOSED.** DA validation is
  skipped only on a *non-secure* chip; the fuse read `0x946` shows DAA is
  enforced here. (Note the bit labels in that file: bit 1 = sbc, bit 2 = daa.)
- **[brom-recovery.md](brom-recovery.md)** — ⛔ **CONFIRMED BRICK — never
  trigger.** The preloader can wipe itself to force BROM download, but bit 8 of
  the fuse is blown, so there is no BROM command handler to fall back to.
- **[lk-emulation.md](lk-emulation.md)** — xrefs solved (scan every 2-byte
  boundary), LibTomCrypt pinned to 1.18.2, and a **bricking warning** for the
  tucert path. ⚠️ Its "lock state is one byte" framing is **retired**: byte
  `+0x59a7` is `androidboot.prod`, and `0xdaf2`/`0xdc70` are
  `amzn_is_restricted()` (1 = restricted). See [fos-flags.md](fos-flags.md).
- **[tucert-primitive.md](tucert-primitive.md)** — **`fastboot flash tucert`
  writes arbitrary bytes into IDME on a locked device with NO root and no
  signature check** — that part stands and was verified on the device. ⛔ But its
  "feeds a DER/X.509 parser" rationale is **retracted**: tucert never reaches a
  DER parser. See [unlock-scheme.md](unlock-scheme.md).
- **[lk-reversing.md](lk-reversing.md)** — **our own LK reversed.** The unlock
  key is unforgeable (RSA-2048, keys extracted). ⛔ Its `dev_flags`/`fos_flags`
  proposal is **closed**: the restriction gate sits inside the flag-test
  primitive, so writing them changes nothing while locked — the planned
  experiment at line 199 is predicted to no-op. See [fos-flags.md](fos-flags.md).
- **[lk-analysis.md](lk-analysis.md)** — analysis of a contemporaneous Amazon
  MT8183 bootloader. Its `dev_flags`/`fos_flags` optimism is superseded by
  [fos-flags.md](fos-flags.md) (correct bits: fos `0x80`, dev `0x40` — and both
  are gated).
- **[root.md](root.md)** — **root achieved**, how to re-obtain it, and the
  firmware dump inventory.
- **[unlock.md](unlock.md)** — bootloader unlock feasibility. Short version:
  **dead**. `oem_unlock_supported=1` tested and disproved; LK has no unlock
  commands; only path with any ceiling is reversing LK.
- **[firmware-sources.md](firmware-sources.md)** — **a 7.4-branch OTA IS
  downloadable** (`cypress` PS7466). Diff targets, the real OTA endpoint, and
  why brute-forcing URLs is impossible.
- **[app-layer-audit.md](app-layer-audit.md)** — full sweep of all 87 APKs.
  **The app layer is closed for the flashing goal**, and there is **no FRP
  partition**, which kills the `oem_unlock_supported=1` thread for good.
- **[network-behavior.md](network-behavior.md)** — what leaves the device on
  Wi-Fi. **RAFT transmits nothing; there is no OTA client at all.**
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

- **2026-08-26**: **v27 — DISPLAY RE-ENABLED; THE GUI WAS NEVER PROVEN BROKEN.**
  Re-reading v21–v24 shows the headless v26 was a diagnostic retreat, not a
  verdict: v24 got `disp_probe` *and* `disp_probe_1` returning 0 once GCE was
  up, and hung one step later in `mtkfb_init` → `m4u_do_mva_alloc` purely
  because `/m4u` was disabled. v25 fixed that and its log was lost, so the
  display-capable config has simply never been observed. The captured live FDT
  settles the question — Fire OS carries **no `status` at all** on `/gce`,
  `/m4u`, `/mtee`, `/devapc` or `/mtkfb@0`. Checked the memory map against
  `reserved-memory`: our DTB sits below the ram_console ring (which is why
  ramoops survived) and nothing overlaps `mblock-6-framebuffer`
  (`0x624e0000..0x64400000`), so the display carveout is intact. Built **v27**:
  `ramdisk-gui-probe.gz` (15,344,648 B, `sha256 47db4777…`) and
  `yacht-live-recovery-gui.dtb` (215,383 B, `sha256 84b121ce…`) with `/mtkfb@0`
  restored to the live default, `androidboot.serialno` added for a stable adb
  handle, and initrd bounds byte-exact. The probe reports fb0 geometry,
  `/sys/class/leds`, `/sys/class/backlight` and `/dev/input` to kmsg **and**
  pmsg **before** its 300 s hold, so a hard reboot still leaves the verdict in
  ramoops. TWRP is still not autostarted — a manual `/sbin/twrp-start.sh` is
  included but referenced by no `.rc`. Also filled a real gap: there was no DTB
  generator checked in, so added `twrp/make-recovery-dtb.sh`, validated by
  regenerating v26 from the live FDT (identical decompiled output; the shipped
  blobs preserve the live strings table, proving `fdtput` in-place edits rather
  than a `dtc` round-trip). `prepare-boot-smoke-ramdisk.sh` gained a
  `YACHT_PROFILE` knob with the default path regression-checked byte-identical
  (`sha256 51b21c48…`). Nothing flashed or staged to the device. Flagged:
  `BoardConfig.mk` sets both `TW_SCREEN_BLANK_ON_BOOT` and `TW_NO_SCREEN_BLANK`
  (remote builder unreachable to confirm against source). See
  [warm-boot.md](warm-boot.md).

- **2026-08-26**: **v26 HEADLESS PAYLOAD AUDITED — NO CORRECTION NEEDED.**
  Re-verified every RAM-only artifact from the files themselves. The v26 DTB
  (`yacht-live-recovery-adb-headless.dtb`, 215,367 B) has `/gce` and `/m4u`
  okay, `/mtee`, `/devapc` and `/mtkfb@0` disabled, and
  `linux,initrd-start/end = 0x55000000..0x55ea231a` — byte-exact against the
  15,344,410-byte ramdisk, with kernel/DTB/initrd placement collision-free.
  `maxcpus=1`, recovery mode, `initcall_debug`/`loglevel=8` and permissive
  bootargs all present; the live `skip_initramfs`/dm-verity tokens correctly
  absent. `prepare-boot-smoke-ramdisk.sh` rebuilt twice is byte-identical
  (`sha256 51b21c48…`) and matches the staged artifact; `prepare-warm-payload.py`
  likewise reproduces bit-for-bit. Ramdisk diff vs stock TWRP is exactly two
  added files; both fstabs are inert; TWRP does **not** autostart (the recovery
  service is `sleep 3600`); and diffing the sanitized MT8183 overlay against the
  real stock overlay confirms the `mmcblk0boot0` preloader symlink and
  `force_ro 0` block is the only thing removed. No block-device or `force_ro`
  command survives anywhere in the ramdisk. Nothing was flashed, staged to the
  device, or rebuilt; `recovery.img` (`sha256 23fd505f…`) and the stock recovery
  backup (`sha256 6ec64a2d…`) are untouched. Open items: bootargs lack
  `androidboot.serialno`, and the smoke script still self-reboots after ~20 s.
  See [warm-boot.md](warm-boot.md).

- **2026-08-25**: **ROOT FAST PATH + LIVE PVT FDT + KERNEL RAM STAGE.** Patched
  the exploit to actually use yacht's verified `modprobe_path` address and to
  retry all three known pages before broad flaky-read hunts. A non-exploiting
  UAPI probe passed, then the first controlled exploit process gained root
  without rebooting despite the known `4/512` read mismatch. Captured yacht's
  exact live 216,003-byte PVT FDT (`sha256 ad340594…`) and built a recovery
  handoff copy with exact initrd bounds. The expanded introspection module
  resolved shutdown, cache and MTK watchdog functions; null private data
  symbols are explained by `CONFIG_KALLSYMS_ALL=n`. Finally, a new
  non-jumping exact-ABI module staged 6,332 Image, 3,760 ramdisk and 53 DTB
  pages in kernel RAM, validated their formats and zero destination overlap,
  then unloaded and freed everything cleanly. No watchdog register, final
  destination, partition or boot flag was written. See [root.md](root.md) and
  [warm-boot.md](warm-boot.md).

- **2026-08-25**: **TWRP BUILDS; direct userspace launch closed by platform
  watchdog.** Corrected Claude's manifest/product errors, built TWRP 9 on
  `erin@10g.warc.zip`, and locally validated the 25,407,488-byte image
  (`sha256 23fd505f…`). Its boot-header geometry matches stock and its embedded
  kernel is byte-identical to the stock recovery kernel. Three RAM-only chroot
  tests used an empty staged fstab (zero block devices exposed) and made no
  flash writes; all reset with `watchdog/watchdog_sw`, including a final
  15-second attempt that armed rollback before suspending `system_server` and
  SurfaceFlinger. The tablet recovered normally each time. Userspace
  coexistence is closed. A new host-side validator safely split the image into
  the arm64 Image, ramdisk, and four self-identified PVT/DVT/EVT/proto DTBs,
  but deliberately leaves the live variant unselected. Booting this valid
  ramdisk now requires the module-carried arm64 warm transition. See
  [twrp/device/amazon/yacht/README.md](twrp/device/amazon/yacht/README.md) and
  [warm-boot.md](warm-boot.md).

- **2026-08-25**: **ROOT REPRODUCED; CUSTOM KERNEL MODULE EXECUTED.** The third
  controlled CVE-2022-38181 attempt succeeded: `uid=0`, `u:r:kernel:s0`,
  SELinux permissive. Recovered yacht's exact enforcing PA `0x419e0668`, live
  `modprobe_path` PA `0x417b51c8`, and all six protected shipping modules.
  Yacht's authoritative ABI is `4.4.146+`, `module_layout=0x62fa6c4c`,
  `printk=0x985558a1`, `kallsyms_lookup_name=0xe007de41`. Built a 4,544-byte
  AArch64 `yacht_hello.ko` on `erin@10g.warc.zip`; live `insmod` returned 0,
  its init message appeared in dmesg, and `rmmod` returned 0 with the exit
  message. Also preserved a 52,887-name root-only kallsyms dump. No partition,
  IDME, or boot flag was written. See [root.md](root.md) and
  [source-kernel.md](source-kernel.md).
  A second read-only `yacht_introspect.ko` also loaded and unloaded cleanly,
  proving private-symbol resolution through `kallsyms_lookup_name()`. It found
  `machine_shutdown=ffffff8008085d5c`,
  `secondary_holding_pen=ffffff8008081e60`, and live IDME helpers. Both kexec
  syscall names resolve to the same unsupported stub, confirming that the
  warm-boot transition must be carried by the module rather than invoked as a
  hidden stock syscall.

- **2026-08-25**: **Transient-root reliability retest: vulnerability present,
  two recoverable kernel reboots, no new root win yet.** `jit_trigger` confirmed
  the JIT `DONT_NEED` primitive. Two `exploit_trona` runs reached eviction,
  aliasing, freed-page mapping and full PGD probe rounds, then rebooted during a
  later allocation attempt. Post-crash state returned normally as enforcing
  shell. The preserved 2026-08-21 `pwned2` is stale and must not be counted as a
  new success. Remote Linux builder `erin@10g.warc.zip` is reachable and has
  Docker; exact-source compilation should move there. See [root.md](root.md)
  and [source-kernel.md](source-kernel.md).

- **2026-08-21**: 🔑 **Unlock system mapped end to end; two new write primitives;
  portability question closed.** Multi-agent sweep, every load-bearing claim
  re-verified against our own binaries.
  - **`fastboot flash tucode` discovered** — the IDME dispatcher at `0xe4c8`
    accepts *two* names (`tucert` → `0x1d9c`, `tucode` → `0x1dbc`), each with its
    own error string. Prior notes listed only `tucert`. Confirmed by reading the
    locked-hw allowlist table at `0x8366c`: `{oem relock, oem flags,
    flash:unlock, flash:tucert, flash:tucode}`.
  - **The scheme:** cert (`AZTU`+base64, 592 B = 336-byte payload + 256-byte RSA
    sig, embedded pubkey at `+0x24`) → phase 1 verifies it against LK's root key
    → phase 2 verifies IDME `t_unlock_code` over one of ten 32-byte codes using
    *the cert's* key. `"Device is temporarily unlocked, %d reboots remaining"`
    takes the matching code index as `%d`.
  - **Codes are an RPMB per-boot nonce.** Delivered as ATAG tag `0x886100A7` in
    the preloader→LK hand-off (magic `LPLP`, blob pointer = `r4` at LK entry);
    zero `LPLP`/`0x8861xxxx` in any writable partition. Derived in the preloader
    at `0x252c` as `HMAC-SHA256(S_device, counter)` from an `AZTU`-magic block in
    **RPMB block 1**, counter incremented every boot. **Not portable, expires in
    ≤10 reboots** — no leaked pair helps anyone.
  - **`fos_flags` bit `0x80` = dm-verity off; `dev_flags` bit `0x40` = SELinux
    permissive** — real switches, but `bl #0xdaf2` sits *inside*
    `fos_flags_test` (`0x2bf28`), so a write changes nothing while restricted.
    `oem flags` permits only `usr_flags`, which nothing reads.
  - **PMIC USBDL commands bypass the whitelist** (`usbdl_pwr_write16`
    `VA 0x205dcc` → `pmic_config_interface`, hardcoded status, no
    `sec_region_check`). The only surviving USBDL primitive; enables rail control
    / glitching without a rig. Read probes safe, rail writes potentially
    terminal.
  - **Whitelist tables read directly:** write `{0x10007000/0x1000,
    0x1001a080/0x4}`, read those plus `{0x11f10000/0x1000}`. Matches the live
    probe exactly. The **eFuse page is read-only**, and `0x1001a080` was never
    probed.
  - **Refuted (recorded so they are not retried):** the `is_in_region` integer
    overflow (our `sec_region_check` opens with a `cmn`/`blo` carry check;
    `is_in_region` has five comparisons vs upstream's two); `CMD_SEND_DA`'s
    missing `da_region_check` (real, but `CFG_DA_RAM_ADDR 0x40200000` overwrites
    the attacker `da_addr`); and the tucert DER theory (tucert never reaches a
    DER parser).
  - **ARB is RPMB-backed, not eFuse-backed**, so downgrade is blocked and
    ARB-clearing is a *product* of preloader code exec, not a route to it. The
    koboreru marker `check_part_overlapped` is **absent** from both preloader
    images.
  - **Scene corrections:** `mustang` is Fire 7 9th gen **MT8163** (wrong family);
    the MT8183 sibling is `maverick`/KFMAWI, unlocked, giving a public signed
    preloader for diffing. No hardware BROM test point exists on this board
    family; `fastbrick` and HeapB8 are inapplicable.


- **2026-08-21**: ⛔ **The USBDL patch plan is DEAD, and the "root-free memory
  write" claim is RETRACTED.** Ran the whole thing against the live device.
  - **Reached the preloader** from software with no buttons, handshaked on a
    locked unrooted device, `HW code: 0x788`. That part works.
  - **Read `efuse 0x11f10060 = 0x00000946` live off the device**, matching the
    `atag,devinfo` derivation exactly. Two independent methods agree.
  - **But memory access is address-filtered.** Probing 15 addresses: only
    `0x11f10000`–`0x11f10100` (eFuse window) and `0x10007000` (WDT) are
    readable. `0x08000000`, DRAM, and the entire preloader code region all
    return status `0x1000`. `WRITE16 0x0022D8B8` returns `0x1001` — **denied**.
    The patch can never be applied over USBDL.
  - **The earlier "CONFIRMED root-free arbitrary memory write" was my error.**
    mtkclient's `Patched "hash_check" in preloader` lines come from
    `patch_preloader_security_da1(self, data)`, which patches a **host-side
    bytearray** — the DA blob being prepared for upload. Nothing was ever
    written to the device. A tool's log line is not an observation of the
    device; it should have taken a read-back to call that confirmed.
  - **Opcode correction:** the dispatcher is `READ16=0xD0, READ32=0xD1,
    WRITE16=0xD2, WRITE32=0xD4`; `0xA2` is a *legacy 16-bit* read. Earlier notes
    saying `0xa1 WRITE16 / 0xa2 READ32` were wrong — caught because `0xA2`
    returned two bytes, not four.
  - **macOS can never use mtkclient's USB path here**: AppleUSBCDC claims the
    interface, so libusb never enumerates `0e8d:2000` at all. Only the serial
    path (`/dev/cu.usbmodem*`) works. Preloader window measured at **~4 s after
    `adb reboot`, lasting ~2–3 s**.
  - Full detail, including protocol framing and resync, in
    [preloader-usbdl.md](preloader-usbdl.md).

- **2026-08-21**: 🔧 **Corrected the fuse bit labels, and fixed a real flaw in
  our own exploit's safety check** — both found offline while the tablet was
  powered off, before the patch ever ran.
  1. **bit 2 of `0x11f10060` is `daa_enabled`, not `sbc_enabled`.** `sbc` is
     **bit 1**. The preloader has *three* near-identical fuse stubs 16 bytes
     apart (`0x2cba8` bit1, `0x2cbb8` bit2, `0x2cbc8` bit0); the caller at
     `0x1ff0a`–`0x1ff1a` calls the first two back to back and stores them to
     security block `+8`/`+9`, which fixes the mapping. Both bits are set, so
     **no conclusion changes** — but the target we are patching is the **DAA**
     gate, which is exactly why the failure reads `DAA_SIG_VERIFY_FAILED`.
     Corrected in [efuse-answer.md](efuse-answer.md).
  2. **The old "oracle 2" could not tell those three stubs apart.** All three
     begin with the identical bytes `0x68184b02`, so a `PL_BASE` wrong by ±16
     would have passed the check and silently patched `sbc_enabled` instead.
     Added **oracle 3**, which reads the `ubfx` word at +4 of the target *and*
     both neighbours (`0x0040f3c0` / `0x0080f3c0` / `0x0001f000` — mutually
     distinct), pinning the address to the byte. Post-patch it also re-checks +4
     is untouched. Verified offline against the dumped image.
  3. Mapped `usbdl_verify_da` end to end. **`0x2cbb8` is the only branch that
     selects secure vs non-secure**, with just two callers image-wide. The
     second `movw r5,#0x7024` at `0x4e48` is reached only from `0x4bf8`
     (`cmp r3,#0xd5`) — the **JUMP_DA** handler — so it is a downstream symptom,
     not an independent gate. Nothing else stands between the patch and an
     accepted DA. **Still untested on hardware.**

- **2026-08-21**: ⭐⭐ **CONFIRMED root-free memory write inside the running
  preloader.** Preloader USBDL turns out to be reachable **from software with no
  buttons** — start mtkclient polling, then `adb reboot`, and the device
  enumerates as `0e8d:2000` "MT65xx Preloader". mtkclient then handshakes and
  reports `Patched "hash_check" in preloader` / `Patched "get_vfy_policy" in
  preloader`, i.e. **`WRITE16`/`WRITE32` are ungated — no SLA**, answering the
  open question in [usbdl-memory-commands.md](usbdl-memory-commands.md). The DA
  upload then fails `DAA_SIG_VERIFY_FAILED (0x7024)` — **exactly the constant we
  had already found at file `0x4dfc` in `usbdl_verify_da`**, so static analysis
  and live behaviour agree. Next step is patching the secure-chip query at file
  `0x2cbb8` → **runtime `0x0022D8B8`** (preloader loads at `0x00200D00`) to
  `movs r0,#0; bx lr`, which forces the *"DA validation disabled on non-secure
  chip"* path. Read oracle for verifying addressing: `READ32 0x08000000` should
  give `0x788`. ⚠️ Device is currently parked in preloader USBDL — **hold power
  ~10 s** to recover; not bricked.

- **2026-08-21**: Mapped the **preloader USBDL command dispatcher** — it accepts
  **`0xa1 WRITE16` and `0xa2 READ32`**, MediaTek's arbitrary memory primitives,
  alongside `SEND_DA`/`JUMP_DA`/`SEND_CERT`/`GET_ME_ID`. This matters because the
  fuse we confirmed (bit 8) disables the **BootROM** command handler, **not the
  preloader's** — and preloader USBDL (`0e8d:2000`) is a separate mode from BROM
  (`0e8d:0003`). Also established that `Tool connection is unlocked` is printed
  **unconditionally** before `bldr_handshake`, so it is a status line rather than
  a gate. Whether SLA additionally gates the memory commands is the open
  question, and there are no SLA challenge strings in the binary. Confirmed the
  preloader→LK security block layout (`+8 sbc_enabled`, `+9 daa_enabled`,
  `+a rpmb_state`, `+b prod_dev`) — the latter two match `ro.boot.rpmb_state=2`
  and `ro.boot.prod=1`, validating it. **Defined a zero-risk read-only test** with
  a perfect oracle: `READ32` of chipid `0x08000000` should return `0x788`, which
  we already know from `devinfo[28]`. See
  [usbdl-memory-commands.md](usbdl-memory-commands.md).

- **2026-08-21**: ⭐ **eFuse question ANSWERED without UART.** The preloader
  builds MediaTek's `devinfo[]` from a table of eFuse register addresses at
  `0x383a0` and passes it to the kernel as `atag,devinfo` — so the fuse values
  were already readable from data we had. `0x11f10060` sits at table index **27**;
  `devinfo[27] = 0x00000946`. **Verified** by index 28 mapping to the chipid
  register and reading back `0x788`, the MT8183 hwcode. Decoding: **bit 2 = 1 →
  SBC enabled → DA validation ENFORCED** (so the unsigned-DA bypass in
  [da-validation.md](da-validation.md) is **closed**), and **bit 8 = 1 →
  `EFUSE_Disable_BROM_CMD` set → BROM download is fused off** on this unit
  specifically, no longer second-hand. ⛔ **Consequently the force-BROM-recovery
  path is a CONFIRMED BRICK, not a risk** — with no BROM command handler, wiping
  the preloader is unrecoverable. Only the root + unsigned-kernel-module soft-mod
  route survives. See [efuse-answer.md](efuse-answer.md).

- **2026-08-21**: Swept a batch of remaining theories, all negative — collected in
  [theories-closed.md](theories-closed.md). `AMZN_PL_VERIFY` is a textbook
  RSA-PSS verify with no flaw; the eng-vs-prod key split is not an entry point
  (the verifier is sound and eng images are Amazon-signed too); recovery sideload
  needs Amazon's OTA key (`otacerts.zip` = one self-signed RSA-2048 cert,
  `CN=Amazon`, valid 2022→2049). Confirmed dm-verity is **AVB 1.0**
  (`android-verity` target, `veritykeyid=f3530e18…`, `verity_enabled=Y`), so a
  modified `/system` fails signature check and defeat needs unlock *or* kernel
  code. `keys` is sparse (21 of 2048 pages); `tee1`/`tee2` are byte-identical.
  Pulled the previously-missing `/system/etc/{security,permissions,sysconfig,init}`
  with root, closing the audit gap. **Kernel module build is blocked on
  toolchain, not concept**: the only public MT8183 tree is 4.4.302 on all
  branches vs our 4.4.146+, and our kernel was built with clang 6.0.2 which
  nixpkgs cannot provide. Way through is **CRC patching** — extract `__kcrctab`
  from our own `boot.img` and patch the module's `__versions` + vermagic.

- **2026-08-21**: Exhausted the read-only routes to the SBC fuse and found the
  clean way in. Tried and ruled out: `/dev/mem` (`CONFIG_DEVMEM` unset), the
  `efusec` device (no attributes), root `/proc/cmdline` (no fuse field),
  `/proc/lk_env` (empty), `atag,masp` (**genuinely 0 bytes**), `dmesg` (no MASP
  lines), debugfs `fuseio` (FUSE *filesystem* logging — false lead), and any
  vendor binary touching `/dev/sec`. `boot_para` magics are
  META/FACT/ADVEMETA/FACTORYM/FASTBOOT/METAFORB — **no USBDL magic**, so there is
  no software path into download mode. **But `/proc/kallsyms` exports
  `masp_hal_sbc_enabled`**, with `/dev/sec` as its driver node — so a kernel
  module can call it directly (or just `ioremap(0x11f10060)`), which is now the
  concrete plan. Also spotted **`masp_hal_set_dm_verity_error`**, a kernel entry
  point that manipulates dm-verity state. Note `kptr_restrict=2` zeroes kallsyms
  addresses even for root, so a module is cleaner than an exploit read primitive.

- **2026-08-21**: **Unsigned kernel modules are loadable.** `CONFIG_MODULES=y`,
  **`CONFIG_MODULE_SIG` is NOT set**, `CONFIG_MODVERSIONS=y`, and `/proc/modules`
  shows `wlan_drv_gen3`/`gps_drv` Live — so with root we can insmod arbitrary
  kernel code. That answers the SBC fuse question on-device (a module that
  `ioremap`s `0x11f10060`), and more importantly puts **runtime dm-verity defeat**
  within reach — a plausible route to a persistently modified `/system` without
  unlocking. Ruled out the easier routes first: **no `CONFIG_DEVMEM`** (so
  `/dev/mem` cannot be created), the `efusec` device exposes no attributes, and
  the full root-read `/proc/cmdline` carries no fuse field. `boot_para` magics
  are `METAMETA`/`FACTFACT`/`ADVEMETA`/`FACTORYM`/`FASTBOOT`/`METAFORB` —
  **no USBDL magic**, so there is no software route into download mode. Also
  confirmed **`androidboot.wpc.support=1`**: the Qi coil *is* fitted, which
  earlier notes hedged on because `wpc_cal` is empty.

- **2026-08-21**: ⭐⭐ **Best lead of the project: DA validation is conditional.**
  `usbdl_verify_da` in the preloader calls a secure-chip query and, if it does
  **not** return 1, prints *"DA validation disabled on non-secure chip"* and
  returns the success value **without checking the signature at all**. The query
  (`0x2cbb8`) is four instructions: read `0x11f10060`, extract **bit 2**. That
  address is `efuse_base + 0x60` on the MT8183 eFuse controller — confirmed live,
  the device exposes `/sys/devices/platform/11f10000.efusec`. **The same register's
  bit 8 is the community-reported `EFUSE_Disable_BROM_CMD`**, so one register
  answers both open questions and our own binary corroborates that second-hand
  claim. If bit 2 is clear, preloader USBDL (`0e8d:2000` — a *different* mode from
  BROM, reachable even where BROM is fused) accepts an **unsigned DA** → full
  flash access with **no root, no unlock, and OTA-proof**. Whether it is clear is
  genuinely undetermined: Amazon runs their own `AMZN_PL_VERIFY` layer
  independent of MTK SBC. **UART settles it for free — the preloader prints
  `sbc_enabled` and `[EFUSE] sbc` at every boot.** No `/dev/mem` and the `efusec`
  device exposes no readable attributes, so on-device reading needs the Mali
  read primitive. See [da-validation.md](da-validation.md).

- **2026-08-21**: **Found a downloadable 7.4-branch OTA** — see
  [firmware-sources.md](firmware-sources.md). `cypress` PS7466 (Fire OS 7.4.6.6,
  Android 9, `amz-p` release-keys, 1.23 GB) is confirmed fetchable and is the
  only public 7.4 artifact in existence — no Fire *tablet* is on 7.4. Good for
  diffing the 7.4-specific framework; useless for LK (it is u-boot). Also 8 more
  `trona` builds are available for an over-time diff on the closest MT8183
  relative. The real OTA endpoint is
  `POST softwareupdates.amazon.com/software/inventory`, but it is **identity-
  gated** — unauthenticated requests never reach deviceType validation, and no
  public tool implements a working manifest client. **Two corrections:** token
  directories are per-(device, build), *not* shared (`Fire_HD10` 200 vs
  `Fire_HD10_Plus` 403 in the same dir), and S3 **masks 404 as 403** so filename
  brute-forcing yields zero signal. Re-ran the endpoint hunt properly with all 90
  APKs and 50 JARs decompressed plus the recovery ramdisk unpacked: still **zero**
  OTA hosts, and `sbin/recovery` is **sideload-only with no network stack**.

- **2026-08-21**: Audited all 87 APKs — see
  [app-layer-audit.md](app-layer-audit.md). **The app layer is closed for the
  flashing goal.** Decisively: **there is no FRP / `persistent_data_block` /
  `seccfg` partition**, so AOSP's `OemLockManager` has no backing store and the
  "OEM unlocking" toggle is inert at *both* ends — that finally closes the
  `ro.oem_unlock_supported=1` thread. `Settings` is stock AOSP; only three
  packages on the image reference IDME/flags and all were already known.
  **`fdrw` is the Factory Data Reset *Whitelist*** (not a KV store) — it writes
  `/cache/recovery/fdrw.conf` and hooks `getExtraFactoryResetBootCommand`, a
  vendor callback that appends arguments to the recovery boot command; whether
  those args are influenced by file contents is **the top unresolved question**
  (AOT code, needs CompactDex conversion). Found a real privesc: **`ArcusProxy`
  is an exported binder with zero permission checks** (config disclosure,
  attribute steering, and `amazon.arcus.*` broadcast injection to all users) —
  though nothing it reaches touches lock state. Also **corrected
  [network-behavior.md](network-behavior.md)**: the lockscreen wallpaper service
  fetches from CloudFront on **every boot**, which the first pass missed.

- **2026-08-21**: ⭐ **Found "Force brom download recovery" in the preloader**
  (`0x24f84`–`0x2504e`) — see [brom-recovery.md](brom-recovery.md). Holding key
  id 0 through a 1500 ms window (plus two internal gates) makes the preloader
  **write 2048 zero bytes over its own `preloader` partition**, destroying the
  EMMC_BOOT header so the BootROM falls through to USB download mode. Amazon
  shipped a deliberate self-brick-to-recover path. **This undercuts the working
  assumption that BROM is fused off** — such a path is worthless, and a
  permanent brick, unless BROM USBDL actually works on this hardware. Not proof,
  but it moves the fuse question from "almost certainly closed" to "must be
  tested". Also noted the preloader prints `sbc_enabled`/`daa_enabled` and
  `[EFUSE] sbc_key_hash` at every boot — **UART reads the fuse state directly,
  risk-free.** ⚠️ Do NOT trigger the recovery path first: if the fuse *is*
  blown, wiping the preloader is an unrecoverable brick.

- **2026-08-21**: **Mapped the preloader's unlock decision end to end** — see
  [lk-emulation.md](lk-emulation.md). It reads IDME at **hardcoded offsets that
  match our parsed table** (`0x4ec` = `unlock_code`, 256 B; `0x2afc` =
  `unlock_version`, 4 B), composes the 26-byte message `"0x"` + three `%08x`
  values, does **one RSA-2048 verify**, and stores the result with
  `clz(r0)>>5` — a branchless exact zero-test — into the lock-state global that
  becomes LK's boot-arg byte. Memoised per boot via a `-255` sentinel.
  **No logic flaw at any step.** This closes the "find a bug in the unlock
  check" line of attack; the verification chain is clean.

- **2026-08-21**: Answered the Wi-Fi question — see
  [network-behavior.md](network-behavior.md). **RAFT is inert** (no INTERNET
  permission; its metrics target `com.amazon.raftsystemservice`, which is not
  installed), and **no OTA client exists on this image**, so an unattended
  update cannot take our root. What does leave: an Amazon-branded captive-portal
  probe, a 24h Arcus config *pull* to `arcus-uswest.amazon.com`, and NTP. The one
  RAFT risk is a DNS SRV lookup for `_kerberos._tcp.ant.amazon.com`, which fires
  **only** if someone types credentials at the lockscreen. Also corrected:
  **`fireosdha` is Device Hardware *Attestation*, not a health agent.**
  Reversed `com.amazon.kor.demo` (present but unregistered) — the retail store
  demo, enterable by cloud push, a search easter egg, or a physical **"Tardis
  Key"** USB. Its USB system-update path is dead here (no OTA package) and would
  be a signed sideload anyway.

- **2026-08-21**: Reversed **Shipmode** (`com.amazon.shpm`) — it is a **factory
  wipe**, not a battery/transport mode. Triggered by the **unprotected**
  broadcast `com.amazon.kindle.otter.shipmode`; runs as system uid; wipes
  `/data/misc/wifi`, deletes `locksettings.db`/`password.key`, **strips `adb`
  from `persist.sys.usb.config`**, sets `vendor.amazon.fos_flags.wipe=1`, then
  shuts down. **Its guard is a no-op** — `PreVerificationTask` logs
  *"ShipMode called when device was provisioned"* but returns `true` on every
  path, so it does not abort on a provisioned device. ⚠️ An accidental broadcast
  would cost us ADB, dev options and the root foothold. See
  [customization.md](customization.md).

- **2026-08-21**: **Static analysis cracked; unlock decision located in the
  PRELOADER.** Solved xref recovery (scan from every 2-byte boundary, validate
  `ldr [pc]`/`add pc` pairs against known strings) — all 12 target strings
  resolved, and the whole image is base 0, so the earlier multi-blob theory was
  wrong. Ghidra's original xrefs were right after all. Found
  `amzn_target_is_unlocked` at `0xe234`: the entire runtime lock state is **one
  byte** at boot-arg struct `+0x59a7`, and **LK only ever reads it** — all four
  references are loads, and the offset exceeds Thumb's immediate range so a write
  would need a `movw` that does not exist. The RSA verification is in the
  **preloader** (`AMZN_UNLOCK`, *"Fail to compose unlock code"*, *"Fail to read
  unlock signature"*, *"rsa2048 public key decryption"*), now extracted with load
  address `0x00200D00`. Separately, LibTomCrypt pinned to **1.18.2** with a
  genuine unbounded-recursion bug (298 levels in 1021 B, ~12–17 KB stack vs LK's
  ~8 KB) and a novel 32-bit `_fetch_length` overflow — but **every write is
  bounded**, so crash/OOB-read only, no code execution.
  ⚠️ **Bricking risk:** tucert is parsed every boot before verification, so a
  crashing blob is a permanent boot loop with no fastboot recovery. **UART is now
  a prerequisite, not a convenience.**

- **2026-08-21**: Bounded the tucert primitive and mapped the blockers. The
  write is **correctly bounded to 1024 bytes** (1025 → `write tucert failed!`,
  nothing written), so there is **no overflow primitive** — any attack must be a
  DER/X.509 **parse** bug. Also ruled out a software route into MediaTek
  download mode: `adb reboot edl` just reboots normally
  (`0x1949:0x0642`, not `0e8d:0003`/`0e8d:2000`). And confirmed there is **no
  boot-property oracle** — `ro.boot.*` is byte-identical with a bogus cert
  installed. **UART is now the gating task**: `oem logcat lk` is refused on
  locked hw, so DER fuzzing would be blind to everything but hard crashes.

- **2026-08-21**: ⭐ **Found an unauthenticated, root-free write primitive.**
  `fastboot flash tucert` accepts arbitrary bytes on a **locked** bootloader and
  writes them into the IDME `t_unlock_cert` field — confirmed by flashing 256
  `0x41` bytes and reading them back from `/proc/idme` after reboot, then
  restoring. `flash unlock` by contrast verifies before writing
  (*"signature length error"* / *"unlock signature verify failed, do nothing!"*),
  so the asymmetry is the bug: tucert defers verification to boot. **Corrects an
  earlier claim** that the locked-hw allowlist was only three getvars — that was
  inferred from `getvar`/`oem` probes; `flash:unlock` and `flash:tucert` are
  permitted too. The 1024-byte field is parsed at every boot by a full
  LibTomCrypt ASN.1/X.509 stack including `der_decode_sequence_flexi`, *before*
  signature validation. Root-free and OTA-proof. See
  [tucert-primitive.md](tucert-primitive.md).

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

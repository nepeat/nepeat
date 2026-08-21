# Rooting `yacht` — plan

**Root looks achievable. A bootloader unlock does not, and therefore neither
does a custom ROM.** Worth saying that plainly up front so effort goes where it
can pay off. What's realistically on the table is **per-boot root, permissive
SELinux, and debloat** — not LineageOS.

See [unlock.md](unlock.md) for why the bootloader is a dead end.

## Step 0 — free checks, done 2026-08-21

```
ro.debuggable    0
ro.secure        1
ro.build.type    user
adb root      -> adbd cannot run as root in production builds
id            -> uid=2000(shell) ... context=u:r:shell:s0
```

So despite being an internal SKU, this is a **production build** — no free
`adb root`. Worth having checked; Amazon proto/dev units reportedly do ship
with root adb, and `ro.oem_unlock_supported=1` made it plausible.

The result that matters:

```
crw-rw-rw- 1 root root 10, 61 /dev/mali0
```

**`/dev/mali0` is world read/write**, i.e. reachable from the `shell` user with
no privileges at all. That is the attack surface for everything below.

The exact kbase version could not be read — `/sys/class/misc/mali0/device/*` is
all `Permission denied` (the visible node names — `js_ctx_scheduling_mode`,
`soft_job_timeout`, `lp_mem_pool_size` — do confirm a Bifrost-era kbase). Pinning
down `rXXpY` needs `KBASE_IOCTL_VERSION_CHECK`, i.e. compiled code. The exploit
does that itself, so this isn't blocking.

## Step 1 — CVE-2024-31317 "SYSTEM USER" — do this first

A single ADB command. Zygote command injection via
`hidden_api_blacklist_exemptions` grants **system UID** (not root) on Fire OS 7
**up to build PS7704** — this device is **PS7401**, so it qualifies. No native
code, so the 32-bit constraint is irrelevant.

What it buys: `pm disable` / `pm uninstall` on protected Amazon packages, custom
launcher, reading system files, and — the important one —
**permanently disabling Amazon's OTA machinery.** Lost on reboot, but the
changes it makes persist.

Do this **before** anything else, so Amazon can't patch away the remaining
options. It directly discharges the standing "do not accept OTAs" caution in
[PROGRESS.md](PROGRESS.md) by removing the mechanism rather than relying on
discipline.

> ⚠️ Botching the `settings delete global hidden_api_blacklist_exemptions` step
> **bootloops the device.** Read the source thread's first post carefully before
> running anything.

Sources: [XDA SYSTEM USER thread](https://xdaforums.com/t/system-user-fire-cube-stick-tv-tablet-ps7704-fireos7-rs8149-fireos8.4759215/),
[AFTVnews](https://www.aftvnews.com/new-fire-tv-exploit-once-again-allows-custom-launchers-disabling-updates-disabling-amazons-app-blacklist-and-more/).

## Step 2 — full read-only dump

Once system/root is available, `dd` every partition — `lk`, preloader
(boot0/boot1), `boot`, `recovery`, IDME, and the small `kb`/`dkb`/`keys`. Purely
read, completely safe, and it is the raw material for all further analysis
including the LK reversing in [unlock.md](unlock.md).

Follow-ups once dumped: `vmlinux-to-elf` on the kernel for kallsyms;
[`liblk`](https://github.com/R0rt1z2/liblk) / `lkpatcher --analyze-policies` on
LK; and grep the preloader for `check_part_overlapped done` as a free viability
check for the koboreru preloader exploit.

Also unblocks the tests parked in [unlock.md](unlock.md): the eMMC
`manfid`/`serial` vs `unlock_version` comparison, partition sizes, and the IDME
HAL's write path.

## Step 3 — CVE-2022-38181 (Mali kbase JIT UAF) — the main effort

This is the strongest candidate, because there is a published exploit for
**exactly this platform**: MT8183, kbase r14p0, kernel 4.4.146, 32-bit
userspace, BROM fused shut, on an Amazon Fire tablet. It yields
`uid=0(root) context=u:r:kernel:s0` plus permissive SELinux.

**https://github.com/ericpardee/fire-hd-ownership** — source, prebuilt ARMv7
binaries, and an engineering handoff document.

Why it plausibly applies here: Amazon fixed CVE-2022-38181 in **Fire OS 7.3.2.9
(June 2024)**, and this kernel was built **2023-12-05** — six months earlier. The
PS74xx train is a separate internal branch that may never have received the fix.
Bifrost r14p0 sits in the affected range (r0p0–r38p1, fixed r40p0), and Amazon
backports individual fixes rather than bumping the Mali major version.

Notes that matter for a port:

- The MTK r14p0 build uses L3 page-table entries `PA | 0x400000000000c1`, **not**
  ARM's reference `0x443` — copy attribute bits verbatim from a live PTE.
- The exploit locates all anchors **at runtime by content scanning** (`init_task`
  via `"swapper/0"`, `modprobe_path` via `"/sbin/modprobe"`, `selinux_enforcing`
  via live avc_cache_stats deltas) rather than by static offsets. That is what
  makes it portable to a build with no published vmlinux — which matters here,
  since Amazon never released kernel source for `pinnacles`. Closest reference is
  the [trona/maverick MT8183 kernel](https://github.com/amazon-mt8183-devs/android_kernel_amazon_mt8183).
- Kernel phys base `0x40080000`; GPU aperture blocked `0x40000000–0x40400000`,
  writable above.
- The kmalloc-128 race crashes ~50% of attempts, each costing a reboot;
  `grind2.sh` automates the loop.
- Post-root, `/system` remount is still blocked by dm-verity, but
  `pm uninstall --user 0` works and persists.

Risk is **kernel panics and reboots, not bricks**, and `/data` is already empty.

Other ARMv7 Fire ports that de-risk the ABI work:
[Raven (FireTV Cube 2)](https://github.com/Pro-me3us/CVE_2022_38181_Raven),
[Gazelle](https://github.com/Pro-me3us/CVE_2022_38181_Gazelle),
[SmileTabLabo (MT8168, Android 9)](https://github.com/SmileTabLabo/CVE-2022-38181),
upstream [github/securitylab](https://github.com/github/securitylab/tree/main/SecurityExploits/Android/Mali/CVE_2022_38181).

**Failure signal:** if `kbase_mem_flags_change` rejects `BASE_MEM_DONT_NEED` on
JIT regions, or the JIT region is never freed by the shrinker, the bug is
patched — stop and go to Step 4.

## Step 4 — CVE-2022-22706 fallback

`kbase_jd_user_buf_pin_pages()` grants write access to CPU read-only page-cache
pages. Bifrost r0p0–r35p0 affected (fixed r36p0, Feb 2022); this device's patch
level is **2022-01-01**, before Android's March 2022 fix.

Attractive because it's a *logic* bug — **no kernel offsets, no heap grooming, no
KASLR defeat** — so it's far more reliable than the 38181 grind. And nobody in
the Fire community appears to have tried it, which makes it the best
odds-per-effort option if Step 3 fails.
[Project Zero RCA](https://googleprojectzero.github.io/0days-in-the-wild/0day-RCAs/2021/CVE-2021-39793.html).

## Ruled out

Stated plainly so nobody re-treads them:

| Candidate | Verdict |
| --- | --- |
| CVE-2019-2215 (binder UAF) | Dead — fixed upstream Feb 2018, *before* 4.4.146 (Aug 2018) existed |
| DirtyPipe (CVE-2022-0847) | N/A — the bug was introduced by the pipe rework in **Linux 5.8**; the code path doesn't exist in 4.4 |
| Dirty COW | Dead — fixed in 4.4.26 |
| mtk-su / CVE-2020-0069 | Dead — patched Fire-wide from ~March 2020; confirmed crashing on 11th gen |
| GhostLock / CVE-2026-43499 | N/A — ports exist only for 5.10+/Fire OS 8 |
| io_uring bugs | N/A — io_uring landed in 5.1 |
| amonet-koboreru | Chicken-and-egg: needs existing write access to `tee1`/`lk`, and has no MT8183 config |

**The 32-bit-only userspace kills essentially every off-the-shelf modern Android
kernel PoC**, since they're all arm64. The Mali family is the exception precisely
because the ARMv7 porting work is already done.

## Host: macOS is fine

mtkclient does run on macOS arm64 (`brew install macfuse openssl`, venv,
`pip3 install --pre --no-binary capstone capstone`, PySide6 + libusb; run under
`sudo`; use kamakiri2, the default, since plain kamakiri fails on Apple Silicon;
Python 3.10+, not the README's 3.9). But the BROM path is near-certainly closed
anyway — and the *likely* path here is adb plus an Android NDK cross-compile,
which is entirely comfortable on macOS arm64.

## ⚠️ Never do

**Do not write `lk`, `preloader` (boot0/boot1), `tee1`, or `boot_para`.** On MTK
the preloader verifies LK unconditionally — Amazon uses the "always verify
regardless of lock state" sec policy — so a patched or unsigned LK simply won't
load, and with BROM fused off **there is no recovery path**. This is a terminal,
unrecoverable brick. The only legitimate use of a patched LK requires *proven*
BROM access.

Also: no firmware downgrade (anti-rollback is armed), and don't bother with a
Magisk-patched `boot` — it will be rejected by LK, and there's no `vbmeta`
partition to blank.

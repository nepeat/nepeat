# Rooting `yacht`

## ✅ ROOT ACHIEVED — 2026-08-21

```
uid=0(root) gid=0(root) groups=0(root) context=u:r:kernel:s0
Permissive
```

CVE-2022-38181 via `ericpardee/fire-hd-ownership`, unmodified, on the first
successful run. SELinux is **Permissive**. All key partitions are dumped,
including `lk`.

### The exploit reported failure when it had actually won

Worth recording, because it nearly cost us the result. `exploit_trona` printed:

```
[C] trigger 0..5: nothing (enforce=1)
[E3] not found (enforce=1)
[D] === CRED ATTACK === [-] calibration failed
[-] kill stage did not land
```

…but the payload script writes its proof to **`/data/local/tmp/pwned2`**, while
the exploit's `pwned()` win-check reads **`/data/local/tmp/pwned`**. Off-by-one
filename. The modprobe usermodehelper had already fired, run as root, and
flipped SELinux to permissive. The chain succeeded and the program said it
failed.

Lesson: with a stateful kernel exploit, check the device rather than trusting
the tool's own verdict. `getenforce` returning `Permissive` was the tell.

### Root shell, reusable

The `rootsh` setuid binary the payload drops **does not work** — `/data` is
`nosuid`. Use the modprobe trigger instead, which is what actually grants
execution:

```sh
# write commands into the usermodehelper target, then fire it
cat > /data/local/tmp/x <<'EOF'
#!/system/bin/sh
<commands run as uid=0 u:r:kernel:s0>
EOF
chmod 755 /data/local/tmp/x
/data/local/tmp/trig          # unknown binfmt -> request_module -> runs x as root
```

`modprobe_path` is overwritten in kernel `.data`, so it **survives until
reboot** but not across one. Re-run the exploit after each boot. Have the
payload `chmod 666` anything you want to `adb pull`, and `chmod 777` the
directory — otherwise the pull silently returns "0 files".

## Firmware dumped

**Raw partitions** (needed root) — in `fw-partitions/` (gitignored), hashes in
[`dumps/partition-manifest.txt`](dumps/partition-manifest.txt):

| image | size | notes |
| --- | --- | --- |
| `lk.img` | 1 MB | **the bootloader — the prize** |
| `preloader_boot0.img` | 8 MB | eMMC boot0 |
| `idme_boot1.img` | 8 MB | eMMC boot1, the IDME region |
| `boot.img` | 33 MB | kernel + ramdisk |
| `recovery.img` | 43 MB | |
| `tee1/tee2.img` | 5 MB each | **byte-identical to each other** |
| `keys.img` | 8 MB | ⚠️ secrets |
| `kb.img` / `dkb.img` | 1 MB each | ⚠️ Widevine / device keybox |
| `misc`, `boot_para`, `nvcfg`, `gpt` | | |

Confirmed our own `lk.img` carries the identical machinery to the trona
reference in [lk-analysis.md](lk-analysis.md) — same full `amzn_*` roster, and
the same `[SELINUX] set to permissive mode by dev_flags`,
`[DM-VERITY] verify off by fos_flags`, `Only usr_flags can be set for a locked
device`. **So the reference analysis transfers directly, and we now have the
real target binary.**

**Readable trees** (no root needed) — 1.3 GB in `fw/`, inventory in
[`dumps/fw-manifest.txt`](dumps/fw-manifest.txt).

Partition sizes, now visible: `kb`/`dkb` 1 MB, `keys` 8 MB, `lk` 1 MB, eMMC
~29 GB.

> ⚠️ `keys`, `kb`, `dkb` and `idme_boot1` hold per-device secrets — keyboxes,
> attestation material and serials. `.gitignore` blocks `fw-partitions/` and
> `*.img`; verified with `git check-ignore`. Commit hashes, never bytes.

## What this does and doesn't buy

Root is **per-boot** and does not unlock the bootloader — see
[unlock.md](unlock.md). `/system` stays read-only under dm-verity. What it does
buy: full partition dumps, `pm uninstall --user 0` (persists across reboots),
permissive SELinux, and the ability to read everything previously blocked.

The interesting question now is whether root can reach `dev_flags` / `fos_flags`
via the IDME HAL, since those are the switches that turn off verity and make
SELinux permissive *at boot* — see [lk-analysis.md](lk-analysis.md).

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

## LIVE RESULTS (2026-08-21)

### The vulnerability is present

`jit_trigger` on this device:

```
[*] UAPI 11.11
[+] FLAGS_CHANGE(DONT_NEED) accepted on JIT region
[*] applying memory pressure, watching MEM_QUERY...
[+] round 23: MEM_QUERY lost the region (Invalid argument)
[+] BUG CONFIRMED: JIT region reclaimed while jit_alloc[] references it
```

**CVE-2022-38181 is unpatched.** The hypothesis held: Amazon fixed it in Fire OS
7.3.2.9 (June 2024), this kernel was built 2023-12-05, and the PS74xx internal
train never received the backport. UAPI **11.11** matches the reference device
exactly (kbase r14p0).

### Correction: aarch64 binaries *do* run here

Worth recording because it nearly derailed this. The reference exploit is
**ELF 64-bit aarch64**, and this device looks 32-bit everywhere you check —
`ro.product.cpu.abilist` is `armeabi-v7a,armeabi`, `abilist64` is empty,
`ro.zygote=zygote32`, `uname -m` reports `armv8l`, and there is no
`/system/bin/linker64` or `/system/lib64`.

I concluded the binaries couldn't run. **That was wrong.** Testing it directly:

```
$ /data/local/tmp/gpu_test
[*] init ok
[*] submitting WRITE_VALUE job, jc=0x7f9f7a9000 target=0x7f9f7a8000
[*] target content = 0x4141414142424242 (expect 0x4141414142424242)
```

The **kernel is arm64 and supports AArch64 EL0**; only the Android userspace is
32-bit-only. Statically-linked 64-bit binaries execute fine, with genuine 64-bit
addresses. So the "32-bit userspace kills every arm64 PoC" concern below applies
to *Android apps*, not to static binaries pushed to `/data/local/tmp`.

`/dev/mali0` is `crw-rw-rw-`, reachable by the `shell` user.

### Exploit progress — primitives work, chain doesn't complete yet

`exploit_trona 0x40080000 root`, no kernel panic, and it gets a long way:

```
[A] hijack write evt=0x1
[A] entry 256 after = 0x41910443 -> *** ALIAS WRITE LANDS ***
[B4] mimic flags=0x400000000000c1 best variant=0
[R] init_task @0x417ad400 cred pair -> 0xffffff80097b65d8
[C] live modprobe_path at 0x417b51c8
[C] modprobe DRAM after = '/data/local/tmp/x'
[C] trigger 0..5: nothing (enforce=1)
[E3] hunting selinux .bss via changing counters... not found (enforce=1)
[D] === CRED ATTACK === [-] calibration failed
```

So on **our** build: the arbitrary-write primitive lands, the PTE format mimic
works, runtime anchor discovery finds a real `init_task` and a live
`modprobe_path`, and the modprobe_path overwrite **succeeds**. What fails is
(a) `selinux_enforcing` is not located by the avc-counter hunt, so SELinux stays
enforcing and the modprobe trigger never fires, and (b) the cred-attack
calibration fails.

Note the anchors differ from the reference device's (`init_task 0x417ad400`,
`modprobe_path 0x417b51c8` here), which is expected — different kernel build.
The `[R]` L2-borrow read reports `4/512 mismatch (READ BROKEN)`, which is likely
why the SELinux hunt can't anchor.

**Status: grinding.** The documented failure mode is a ~50% race loss per
attempt, so repeated attempts are the intended workflow. If it converges,
root follows; if the `[R]` read stays broken, the SELinux stage needs porting to
this build rather than more attempts.

## Firmware dump — done (unprivileged)

1.3 GB pulled over plain adb with no root, into `fw/` (gitignored). Hashed
inventory of the Amazon-specific artifacts is in
[`dumps/fw-manifest.txt`](dumps/fw-manifest.txt).

| tree | size |
| --- | --- |
| `/system/lib` | 705 M |
| `/system/framework` | 208 M (incl. `fosframework.jar` + `boot-fosframework.oat/.art/.vdex`) |
| `/system/priv-app` | 161 M (incl. `RaftSystemUI.apk`, `Shipmode`, `ArcusListener`) |
| `/system/app` | 148 M |
| `/vendor` | 90 M (incl. the `fireos.hardware.*` HALs) |
| `/system/bin` | 3.4 M |
| `/system/etc` | 1000 K |

That covers everything readable without privileges. **Still needs root:** the
raw partitions — `lk` (`mmcblk0p5`), preloader (boot0), IDME (boot2), `boot`,
`recovery`, and `keys`/`kb`/`dkb`.

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

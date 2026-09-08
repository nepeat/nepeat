# Yacht arm64 warm-boot plan

## Why this is now the boot path

The TWRP 9 image is built and structurally valid, but Amazon LK will not load an
unsigned recovery. A live Android chroot is also closed: three empty-fstab
smoke tests reset through `watchdog/watchdog_sw`, even when rollback was armed
before suspending both `system_server` and SurfaceFlinger.

The remaining RAM-only route is a small kexec-style transition carried by an
unsigned module after transient root. This is possible because yacht accepts
our exact-ABI modules and exports GPL `kallsyms_lookup_name()`.

## Known target layout

The final TWRP image reuses the stock kernel byte-for-byte:

```text
kernel:  Image.gz-dtb, sha256 4852ece1022ff2d6ad41e746dad1e7448a3e3c2afca4036d62f4cc2e5fc1a463
image:   twrp/out/recovery.img, sha256 23fd505f2a27f7191d8daa3866a0ed2500f9882aef68e4606a452142c5f58b27
kernel physical address:  0x40080000
ramdisk physical address: 0x55000000
DTB physical address:     0x54000000
```

Do not copy over those final addresses while the current kernel is executing.
Stage the uncompressed arm64 `Image`, ramdisk and selected/patched DTB in normal
pages first; a final identity-mapped relocation stub copies them only after
devices and secondary CPUs are quiesced.

`twrp/prepare-warm-payload.py` now performs the non-destructive host half of
that preparation. It verifies the exact recovery and embedded-kernel hashes,
boot-header geometry, arm64 Image magic and appended-FDT boundaries before
writing a JSON manifest. It refuses to overwrite an output directory and does
not select a DTB unless an index is explicit:

```bash
python3 twrp/prepare-warm-payload.py \
  twrp/out/recovery.img twrp/out/warm-payload
```

The validated split produced a 25,932,040-byte uncompressed `Image`, the
15,397,617-byte ramdisk (`sha256 59d290a5…`), and four DTBs identified as
`pvt`, `dvt`, `evt`, and `proto` at indices 0 through 3.

The 2026-08-25 root window captured yacht's exact 216,003-byte live FDT
(`sha256 ad340594…`): root `version=pvt`, definitively selecting appended DTB
index 0. LK's runtime mutations explain why its full hash differs from the
base DTB. A recovery copy preserves the live RAM/reservation/ATAG data while
setting `linux,initrd-start=0x55000000`,
`linux,initrd-end=0x55eaf2f1`, and recovery/permissive bootargs; its hash is
`627ed5ff…`.

## Exact-source prerequisites

Yacht's Amazon 4.4.146 tree provides `machine_shutdown()`, which calls
`disable_nonboot_cpus()`, but contains no arm64 `machine_kexec.c` or
`relocate_kernel.S`. The transition must therefore supply the small MMU-off
relocation path itself.

The MT8183 watchdog is TOPRGU at physical `0x10007000`. Its `WDT_MODE` register
is offset `0x00`; bit 0 enables the watchdog and writes require key
`0x22000000`. `CONFIG_WATCHDOG_NOWAYOUT` is off. The eventual jump path must
disable this watchdog before Android is quiesced; the chroot tests prove that
leaving it armed resets the tablet before a handoff can finish.

The extended read-only introspection module now probes these names before any
jump code is written:

```text
machine_shutdown
device_shutdown
migrate_to_reboot_cpu
kernel_restart_prepare
smp_send_stop
__flush_dcache_area
idmap_pg_dir
secondary_holding_pen{,_release}
mtk_wdt_{stop,ping}
watchdog_stop
```

It builds with only the three expected ABI imports (`module_layout`,
`kallsyms_lookup_name`, `printk`). Remote artifact:

```text
/home/erin/ai_re-yacht-build/introspect/yacht_introspect.ko
sha256 a4ef2f1f3d73b6e49648cff306cfddd2fa6e0209e1d4a0a1b6f40491ab886beb
```

Live results resolve every shutdown/cache/watchdog function, including
`machine_shutdown`, `device_shutdown`, `migrate_to_reboot_cpu`,
`kernel_restart_prepare`, `__flush_dcache_area`, `mtk_wdt_stop`, and
`mtk_wdt_ping`. `idmap_pg_dir`, `secondary_holding_pen_release`, and generic
`watchdog_stop` return null because yacht has `CONFIG_KALLSYMS_ALL` disabled:
private data objects are excluded. The exact MTK source confirms
`mtk_wdt_stop()` requires a private `struct watchdog_device *`; the final
trampoline should instead map TOPRGU `0x10007000` and clear `WDT_MODE_EN` with
key `0x22000000` directly.

## Non-jumping staging result

The three payloads were copied into a 64 MiB tmpfs on yacht, made read-only and
verified against their host hashes. Temporary `/data` transfer copies were
then removed. `kernel-modules/stage/yacht_stage.ko` resolves all operational
APIs dynamically so its only ABI imports are the already-proven
`module_layout`, `kallsyms_lookup_name`, and `printk`.

Live `insmod` duplicated every payload into kernel-owned noncontiguous pages,
validated the arm64 Image, gzip and FDT headers, enumerated physical pages, and
proved no staging page overlaps a final destination:

```text
Image:   6332 pages, overlaps=0
ramdisk: 3760 pages, overlaps=0
DTB:       53 pages, overlaps=0
STAGED OK; no destination write or branch performed
```

`rmmod` freed all pages cleanly. The verified tmpfs payload remains at
`/mnt/yacht-stage` until unmounted or rebooted; no staging module remains
loaded.

## Staged implementation gates

1. **Read-only prerequisite probe — complete.** Function prerequisites and the
   reason private data symbols are absent are confirmed live.
2. **Host-side payload preparation — complete.** Exact live PVT FDT captured,
   recovery copy patched, payload bounds and hashes validated.
3. **Module dry run — complete.** All 10,145 payload pages staged and freed;
   content and non-overlap checks passed. No CPU shutdown, watchdog write,
   destination copy or branch occurred.
4. **Relocation rehearsal without branch.** Build the identity-mapped control
   page and page-copy list, verify every destination is covered exactly once,
   and report the would-be entry/DTB registers. Still return normally.
5. **Jump milestone, separately authorized.** Migrate to the reboot CPU, call
   `kernel_restart_prepare()` and `device_shutdown()`, disable TOPRGU, call
   `machine_shutdown()`, mask interrupts, clean caches, enter the control page,
   disable the MMU, copy payloads, set `x0=dtb` and `x1=x2=x3=0`, then branch
   to the Image entry.

The first four gates are non-jumping and recoverable. Gate 5 can crash/reboot
and, if DMA is not fully quiesced, could corrupt storage; do not collapse it
into an earlier test.

## v26 headless payload — full artifact audit (2026-08-26)

Independent re-verification of every RAM-only artifact, done by reading the
files rather than trusting the running notes. No device was touched; every
check is host-side and read-only.

### Recovery image and split (unchanged, still valid)

| artifact | size | sha256 |
| --- | --- | --- |
| `twrp/out/recovery.img` | 25,407,488 | `23fd505f2a27f7191d8daa3866a0ed2500f9882aef68e4606a452142c5f58b27` |
| embedded gzip kernel blob | 10,004,528 | `4852ece1022ff2d6ad41e746dad1e7448a3e3c2afca4036d62f4cc2e5fc1a463` |
| decompressed arm64 `Image` | 25,932,040 | `772548f7dd1bd81d48278b6350822d09f3bff07b92259a1caac4a0d208fc9c4c` |
| stock `ramdisk.gz` (TWRP) | 15,397,617 | `59d290a5449941620cde41f8b6afa4ebf4d75466f33b901c4539103470084118` |
| **stock recovery backup** `fw-partitions/dump/recovery.img` | — | `6ec64a2df11c3070dc11beb09bf9e96733d99d2f41e326e3e9858262c88ccd3d` |

Boot header: v1, header_size 1648, page_size 2048, second_size 0, kernel
`0x40080000`, ramdisk `0x55000000`, second `0x40f00000`, tags `0x54000000`.
Four appended DTBs (pvt/dvt/evt/proto), 184,739 / 184,990 / 185,432 / 185,429
bytes. `prepare-warm-payload.py` run twice into fresh directories produced
byte-identical trees, and both match the staged `out/warm-payload/`.
The stock recovery dump is untouched and still hashes to its dump-time value.

### v26 DTB — `out/warm-payload/yacht-live-recovery-adb-headless.dtb`

215,367 bytes. This DTB derives from the *live device* FDT (`out/live/yacht-live.dtb`),
not from the recovery image's appended DTBs — hence 215 KiB rather than 185 KiB.

| node | status | required |
| --- | --- | --- |
| `/gce` | `okay` | ✅ |
| `/m4u` | `okay` | ✅ |
| `/mtee` | `disabled` | ✅ |
| `/devapc` | `disabled` | ✅ |
| `/mtkfb@0` | `disabled` | ✅ |

v26 is exactly v25 (`…adb-diagnostic-gce-m4u.dtb`) plus the `/mtkfb@0` status
property; that node had no `status` at all in v25, so it defaulted to okay.

`/chosen/linux,initrd-start = 0x55000000`, `linux,initrd-end = 0x55ea231a`.
Delta = 0xEA231A = **15,344,410 bytes**, byte-exact against
`ramdisk-adb-smoke.gz`. Placement is collision-free inside the single
`/memory` bank at `0x40000000` size `0x100000000`:

```
kernel  0x40080000..0x4193b108  (25,932,040)
dtb     0x54000000..0x54034947  (   215,367)
initrd  0x55000000..0x55ea231a  (15,344,410)
```

Bootargs (all required tokens present): `maxcpus=1`, `androidboot.mode=recovery`,
`androidboot.force_normal_boot=0`, `initcall_debug ignore_loglevel loglevel=8`,
`androidboot.selinux=permissive`, `enforcing=0`, `buildvariant=eng`.
The live-boot tokens `skip_initramfs rootwait ro init=/init root=/dev/dm-0
dm="…android-verity…" androidboot.veritymode=eio` are correctly *absent*, so
the initramfs is used and dm-verity is never constructed.
`/toprgu@10007000` carries no `status` (defaults okay), so `/dev/watchdog` will
exist and `watchdogd` will not enter a critical restart loop.

### Ramdisk `ramdisk-adb-smoke.gz` — reproducible

Rebuilt twice from `ramdisk.gz` via `prepare-boot-smoke-ramdisk.sh` into a
scratch directory. Both runs: 15,344,410 bytes,
sha256 `51b21c48c6df7671e034caab9f99b64ca6590d6bbab72f391463701f73d59f95`,
`cmp` byte-identical to each other **and** to the staged
`out/warm-payload/ramdisk-adb-smoke.gz`. `normalize-newc.py` is doing its job:
inode renumbering plus zeroed `rdevmajor`/`rdevminor` removes the only
filesystem-dependent fields, and `touch -h -t 197001010000` plus `gzip -9n`
removes timestamps.

3,424 cpio entries. Delta against the stock TWRP ramdisk is exactly two added
files — `./init.recovery.mt8183.rc` and `./sbin/yacht-smoke.sh` — plus in-place
content edits to `init.rc`, `init.recovery.service.rc` and the two fstabs.

| member | type | mode | size |
| --- | --- | --- | --- |
| `./etc/recovery.fstab` | file | 0644 | 56 |
| `./system/etc/twrp.fstab` | file | 0644 | 56 |
| `./init.recovery.mt8183.rc` | file | 0644 | 2051 |
| `./init.recovery.service.rc` | file | 0750 | 72 |
| `./sbin/yacht-smoke.sh` | file | 0750 | 314 |
| `./sbin/recovery` | file | 0750 | 589,356 |
| `./sbin/adbd` | file | 0750 | 1,288,300 |
| `./sbin/sleep` | symlink → `toybox` | — | 6 |
| `./sbin/watchdogd` | file | present | — |
| `./init` | file | 0750 | 1,609,216 |

Both fstabs contain only `# yacht first-boot smoke test: no block devices exposed`.

**Overlay sanitisation confirmed by diff against the real stock overlay**
(extracted from `fw-partitions/dump/recovery.img`). The removed block is
precisely:

```
on property:ro.vendor.mtk_emmc_support=1
    symlink /dev/block/mmcblk0boot0 /dev/block/platform/bootdevice/by-name/preloader
    write /sys/block/mmcblk0boot0/force_ro 0
```

Also dropped: the `ro.debuggable=0` `udc/musb-hdrc/device/cmode` write and the
unused `vendor.usb.acm_*` props. Everything else — configfs gadget setup, the
HID default, the `sys.usb.ffs.ready=1` switch to `ffs.adb`, and `watchdogd` —
is preserved verbatim. Grepping every `.rc` and `sbin/*.sh` in the built
ramdisk for `force_ro|mmcblk|/dev/block|blkdiscard|mkfs|dd ` returns **nothing**.

### Init ordering (traced, not assumed)

`init.rc` line 8 imports `/init.recovery.${ro.hardware}.rc`; bootargs set
`androidboot.hardware=mt8183`, so our overlay is loaded. `on init` creates
`/config/usb_gadget/g1/functions/ffs.adb` strictly before `init.rc`'s `on fs`
mounts functionfs at `/dev/usb-ffs/adb`, which is the ordering adbd needs.

`init.recovery.service.rc` (imported at line 6) previously held
`service recovery /sbin/recovery`; it is replaced by
`service recovery /sbin/sleep 3600`. It carries no `disabled`, so
`class_start default` starts it and PID 1 stays alive instead of TWRP exiting
on the empty fstabs and triggering an init reboot. **TWRP does not autostart** —
task 8 satisfied.

`init.recovery.usb.rc` is still imported and writes to `/sys/class/android_usb/…`,
which does not exist on this configfs kernel. Those writes fail and are logged;
they touch no block device and are harmless.

`init.recovery.hlthchrg.rc` defines a `critical` `/charger` service and `/charger`
is absent from the ramdisk — but nothing imports that file, so it never runs.

### Findings / remaining risks

1. **No `androidboot.serialno` in the recovery bootargs.** The live device
   passes `androidboot.serialno=G002G402413303RL`; our recovery cmdline omits
   it, so the overlay writes an empty
   `/config/usb_gadget/g1/strings/0x409/serialnumber`. The gadget still
   enumerates and adb still works, but the host will address the device by USB
   path rather than a stable serial. Not a boot blocker; a one-token DTB delta
   fixes it if a stable `adb -s` handle is wanted.
2. **The smoke ramdisk self-reboots after ~20 s.** `yacht-smoke.sh` sleeps 20
   then `setprop sys.powerctl reboot`. That is correct for the v26 *diagnostic*
   run — it bounds the exposure and returns control before the module watchdog
   can route through stock recovery and overwrite ramoops — but it is not a
   usable interactive shell. A follow-up "hold" ramdisk (drop the reboot, or
   raise the sleep) is the step after v26 shows userspace entry.
3. **`printk.disable_uart=1` is retained** from the stock cmdline, so UART is
   silent; ramoops/pstore remains the only log channel. v23/v24 prove that path
   works, so this is accepted rather than changed.
4. **Missing imports** `init.recovery.logd.rc` and `init.recovery.vold_decrypt.rc`
   do not exist in the ramdisk. Pre-existing in the base TWRP build, not
   introduced here; init logs a parse error and continues.
5. **v26 modules are built but unstaged.** `yacht_boot_v26_rehearsal.ko` and
   `yacht_boot_v26_jump.ko` exist in `kernel-modules/boot/out/`; nothing has
   been copied to the device.

No correction to the ramdisk or DTB was needed, so **`recovery.img` was not
rebuilt** and all original recovery images and DTBs are untouched.

### Next RAM-only test procedure (v26)

Every step is RAM/tmpfs only. Nothing writes recovery, boot, misc, boot_para,
GPT, or any block device; no `force_ro` is touched.

1. Boot the tablet normally and re-acquire root read-only (CVE-2022-38181).
2. Stage to `/data/local/tmp` **only**:
   `warm-payload/Image`, `warm-payload/ramdisk-adb-smoke.gz`,
   `warm-payload/yacht-live-recovery-adb-headless.dtb`,
   `kernel-modules/boot/out/yacht_boot_v26_rehearsal.ko`,
   `yacht_boot_v26_jump.ko`. Verify the three payload hashes on-device against
   the table above before proceeding.
3. `insmod yacht_boot_v26_rehearsal.ko` first. Require: all pages staged,
   `overlaps=0` for Image/ramdisk/DTB, and a clean `rmmod`. Do not continue on
   any non-zero overlap.
4. `insmod yacht_boot_v26_jump.ko`. Expect the screen to stay dark — `/mtkfb@0`
   is disabled by design.
5. Wait for the scripted reboot (~20 s in target userspace plus init time). If
   nothing happens within ~90 s, hard-reboot. **A hard reboot lands in stock
   recovery first**; that stock UI is not evidence of anything. Let it continue
   into Fire OS before reading logs.
6. Re-root, then immediately capture `/sys/fs/pstore/console-ramoops*` and
   `/proc/last_kmsg` to `twrp/out/warm-boot-logs/v26-console-ramoops` and
   `v26-last-kmsg` **before** anything else reboots the device — that is how the
   v25 log was lost.
7. Success criterion for v26 is a ramoops line
   `yacht-smoke: target userspace entered`, plus `yacht-smoke: setenforce …`.
   Kernel-only progress past `mtkfb_init` without that marker means init ran but
   userspace stalled — check for `init: ` lines and the configfs writes.
   Reaching stock recovery, or any Fire OS log, proves nothing.
8. Only after that marker appears: build the "hold" ramdisk variant (no
   `sys.powerctl reboot`), confirm a stable `adb shell`, and only then launch
   `/sbin/recovery` by hand.

## v27 — turning the GUI back on (2026-08-26)

### Why the display was off, and what the logs actually prove

Re-reading v21–v24 changes the picture. The headless choice in v26 was a
*diagnostic* retreat, not a verdict on the display stack:

| ver | last PC / failure | cause |
| --- | --- | --- |
| v21/v22 | `__const_udelay` after `[DEVAPC] Access Violation Slave: efuse_top` | DEVAPC violation storm resets the warm boot |
| v23 | `die+0x208`, NULL deref in `disp_probe_1` | GCE disabled |
| v24 | `m4u_do_mva_alloc + 0x88` (hang) | `/m4u` disabled, so the IOMMU device never probed |
| v25 | **unknown — ramoops lost** | — |
| v26 | not yet booted | headless fallback |

v24's log shows `disp_probe` **and** `disp_probe_1` both returning 0 once GCE
was up; the hang is one step later, inside `mtkfb_init` → `m4u_do_mva_alloc`,
allocating the framebuffer's IOMMU mapping. Note `MTK_M4U_Init` itself returned
0 — the *driver* initcall runs regardless, it is the *device* that was missing
because `/m4u` was `status = "disabled"`. v25 restored `/m4u` and is therefore
already the display-capable configuration; **its result was never observed.**

So there is no evidence the GUI is broken. There is only missing evidence.

### The live device settles the configuration question

The captured live FDT (`out/live/yacht-live.dtb`) carries **no `status` property
at all** on `/gce`, `/m4u`, `/mtee`, `/devapc` or `/mtkfb@0` — Fire OS boots the
panel with every one of them at their default `okay`. Every `disabled` in our
recovery DTBs is a warm-boot workaround we added, not a device property.

### Memory map — checked, and clean

`/proc/iomem` plus the live `reserved-memory` node:

```
ram_console-reserved-memory   0x54400000..0x54410000   (ramoops ring)
pstore-reserved-memory        0x54410000..0x544f0000   (console-ramoops)
minirdump-reserved-memory     0x544f0000..0x54500000
mblock-1-atf-reserved         0x54600000..0x54640000
mblock-6-framebuffer          0x624e0000..0x64400000   (31.1 MiB)
mblock-5-mtktee-reserved      0x64400000..0x6d200000   (142.0 MiB)
```

Our DTB (`0x54000000..0x54034957`) sits below the ram_console ring — which is
exactly why v23/v24 ramoops survived — and the initrd
(`0x55000000..0x55ea2408`) lands in free RAM between the ATF reserve and the
framebuffer carveout. **No payload overlaps `mblock-6-framebuffer`**, so the
display carveout LK drew the boot logo into is intact for `mtkfb`. Verified
programmatically against all six regions: zero collisions.

### New tooling

`twrp/make-recovery-dtb.sh` — there was no DTB generator checked in at all;
v15–v26 were produced ad hoc. The blobs preserve the live strings block byte
for byte (`strsz=18779`, identical to live), which proves they were made with
in-place `fdtput` edits, **not** a `dtc` round-trip — a round-trip rewrites the
strings table and is not equivalent. The script reproduces that method and was
validated by regenerating v26 from the live FDT: same 215,367 bytes, and
`dtc`-decompiled output **identical** to the shipped blob (6 bytes differ in the
raw struct block, purely property-insertion order).

`twrp/prepare-boot-smoke-ramdisk.sh` now takes `YACHT_PROFILE=smoke|gui`
(default `smoke`) and `YACHT_HOLD_SECONDS`. The default path is unchanged and
was regression-checked: it still emits
`sha256 51b21c48…`, byte-identical to the shipped v26 ramdisk.

### v27 artifacts

| artifact | size | sha256 |
| --- | --- | --- |
| `out/warm-payload/ramdisk-gui-probe.gz` | 15,344,648 | `47db47776e1692aa1d5046caf1992e1d56c88f4ef0ec6097c20568561f72755f` |
| `out/warm-payload/yacht-live-recovery-gui.dtb` | 215,383 | `84b121ce7d8ce88ee72ce9e0dfcf1ef9678a21a5e5dd296ebbcf34da5048dd01` |

DTB: `/gce` okay, `/m4u` okay, `/mtee` disabled, `/devapc` disabled,
**`/mtkfb@0` status removed entirely** so it matches the live default rather
than being force-enabled. `linux,initrd-start/end = 0x55000000..0x55ea2408`,
delta 15,344,648 — byte-exact against the ramdisk. `maxcpus=1` retained
deliberately: display is the only variable being changed this round.
`androidboot.serialno=G002G402413303RL` added, fixing the empty USB gadget
serial noted in the previous audit, so adb gets a stable handle.

Ramdisk (built twice, byte-identical): 3,425 entries — v26's set plus
`./sbin/twrp-start.sh` (0750). Both fstabs still inert, recovery service is
still `sleep 3600`, and **no `.rc` file references `twrp-start.sh`** — TWRP is
still not autostarted, per the standing gate. No `force_ro`, `mmcblk`,
`/dev/block`, `dd`, or `mkfs` anywhere in the payload.

The probe emits its display verdict to `/dev/kmsg`, `/dev/pmsg0` and
`/tmp/yacht-gui-probe.txt` **before** the hold, so a hard reboot mid-window
still leaves the answer in ramoops. It reports:

* `/dev/graphics/fb0` and `/dev/fb0` presence,
* `fb0` `name`, `virtual_size`, `bits_per_pixel`, `stride`, `blank`, `modes`,
* the contents of `/sys/class/leds` and `/sys/class/backlight`,
* `/dev/input/*`.

That last pair matters because `TW_BRIGHTNESS_PATH :=
/sys/class/leds/lcd-backlight/brightness` is baked into the built binary
(confirmed by `strings` on `sbin/recovery`) but has never been checked against
this device — a wrong path is a black screen with a working framebuffer.

### Open risks

1. **v25/v27's configuration is still unproven.** The display may hang somewhere
   past `m4u_do_mva_alloc`. If it does, adb dies with it and we are back on
   ramoops — hence diagnostics-before-hold.
2. **DEVAPC stays disabled.** The v22 violation was an `efuse_top` read, which
   is plausibly benign, but re-enabling DEVAPC is a separate experiment; do not
   fold it into the display test.
3. **`BoardConfig.mk` sets both `TW_SCREEN_BLANK_ON_BOOT := true` and
   `TW_NO_SCREEN_BLANK := true`.** These are opposing knobs. I could not reach
   the remote builder (`ssh erin@10g.warc.zip` timed out) to confirm the
   semantics against TWRP source, so this is flagged, not fixed — but check it
   before blaming the DTB for a blank panel.
4. **300 s hold.** Longer than v26's 20 s so the panel can actually be looked
   at. Safe: the fstabs expose nothing, `watchdogd` is running against the real
   `/dev/watchdog` (present on the live device, and `/toprgu@10007000` has no
   `status`), and the script still ends in a controlled `sys.powerctl reboot`.
5. **No UART.** This board's UART pads are firmware-disabled, so
   `printk.disable_uart=0` buys nothing; ramoops and adb remain the only
   channels.

### v27 test procedure (RAM-only)

Unchanged safety envelope: nothing writes recovery, boot, misc, boot_para, GPT
or any block device, and no `force_ro` is touched.

1. Normal boot, re-acquire root read-only (CVE-2022-38181).
2. Stage to `/data/local/tmp` only: `Image`, `ramdisk-gui-probe.gz`,
   `yacht-live-recovery-gui.dtb`, `yacht_boot_v26_rehearsal.ko`,
   `yacht_boot_v26_jump.ko`. Verify the two v27 hashes on-device first.
3. `insmod yacht_boot_v26_rehearsal.ko` — require `overlaps=0` on all three
   payloads and a clean `rmmod` before going further.
4. `insmod yacht_boot_v26_jump.ko`.
5. **Watch the panel.** Unlike v26 this run may light it up. A TWRP UI is *not*
   expected — `/sbin/recovery` is not started — so a blank-but-backlit panel,
   or the kernel logo, is a pass for this stage.
6. Try `adb devices` from the host; the device should now appear as
   `G002G402413303RL`. If it does: `adb shell cat /tmp/yacht-gui-probe.txt` is
   the full display verdict.
7. Only if `/dev/graphics/fb0` is PRESENT and a backlight path exists, and only
   after the adb shell has proven stable, run `adb shell /sbin/twrp-start.sh`
   by hand. Expect it to exit quickly — the fstabs are empty by design; that is
   a UI test, not a storage test.
8. If the panel stays dark and adb never appears, hard-reboot, let it pass
   through stock recovery into Fire OS **without** re-rooting first if
   possible, and capture `console-ramoops`/`last_kmsg` to
   `twrp/out/warm-boot-logs/v27-*`. The `yacht-gui:` lines are written in the
   first second of userspace, so they will be there if userspace was reached.

# 2026-11-10 — ipd `legacy_abi=1`: why "Node 0 couldn't be created", and the fix

Live debugging on the switch (NixOS, mainline 7.3.3, patched Marvell ipd
0.16.2 loaded with `legacy_abi=1`, vendor SDK 0.1.7 in the ONIE chroot).
Follow-up to [2026-11-10-ipd-source-compat.md](2026-11-10-ipd-source-compat.md).

Marking: **H** = verified on the hardware in this session, **V** = verified
offline (disassembly / source / file contents), **I** = inferred.

## Result

**Node 0 is created and all front-panel netdevs exist** (H). One root cause,
one two-line driver change. `innovium.user` is running on the switch now.
One open anomaly on the CPU RX path is described under "Still failing".

## Root cause: libifcs compares the driver version string with its own

`_ii_pcie_attach` in `libifcs.so` (0x166d234), right after `IPD_QUERY_NODE`
returns (V, disassembly `scratchpad/ipd-agent/agentD/ifcs.dis`):

1. `ifcs_get_version(&maj, &min, &patch)` — reads three words at 0x236ab50;
   the file holds `(0, 12, 41, 1, 0, 1)` (V).
2. takes `query.version` (offset 0x50 of the legacy 112-byte struct),
   replaces every `.` with a space, `sscanf(version, "%d %d %d", ...)`
   (format string at 0x18637f0).
3. if any of the three differs: logs
   `"%s IFCS SDK version(%d:%d:%d) mismatch with driver version(%d:%d:%d)"`
   (0x1863800, only at log level > 1, hence nothing on the console) and
   returns 0x10 = `IFCS_DRIVER_ERROR` (0x166d7d4-0x166d82c). No mmap and no
   further ioctl is attempted.

The patch returned upstream's `ipd_version` = `"0.16.2.0.0.0"`; the vendor
`ipd.ko` returned `"0.12.41.1.0.1"` (its banner in the stock dmesg). 0.16.2 != 0.12.41,
so attach failed. The earlier review listed this check as "not checked,
unlikely to be fatal"; it is fatal.

strace before the fix (H; `evidence/mainline-sdk-2026-11-10/ipd-legacy-strace-before-fix.txt`):

```
15180 openat(AT_FDCWD, "/dev/ipd", O_RDONLY) = 5
15180 ioctl(5, _IOC(_IOC_READ, 0x49, 0x1, 0x8), 0xffffda0f5280) = 0      # INFO_NODES
15180 close(5)
15180 openat(AT_FDCWD, "/dev/ipd", O_RDWR) = 7
15180 ioctl(7, _IOC(_IOC_READ|_IOC_WRITE, 0x49, 0x2, 0x8), 0xffffda0f4368) = 0   # QUERY_NODE
15180 --- SIGSEGV {si_signo=SIGSEGV, si_code=SEGV_MAPERR, si_addr=0x13514} ---
```

The SIGSEGV is a consequence, not a cause: after the failed attach the shell
goes on to run `misc_config.cmd`, whose PCI register writes dereference the
never-mapped BAR0 pointer (NULL + register offset; 0x13514 is in the range of
the LED registers the script writes, e.g. `LED_REFRESH_PRECLIFF` 0x13538) (I).
In daemon mode `innovium.user` simply `exit_group(1)`s after the same QUERY.

Everything else the suspects list named turned out fine (H, after the fix):
the other fields of the translated query struct are accepted, and every mmap
succeeds.

## Change to the patch

`/home/erin/nepeat/nix/nixos/module/cs8210/kernel/patches/ipd-legacy-abi.patch`
(regenerated as a clean `diff -Nurp` of pristine `drivers/ipd` @ 53d8dcf
versus the patched tree; both file names are now relative to `drivers/ipd`,
timestamps dropped). The only functional difference to the previous version:

```c
/* 0.12.x libifcs refuses to attach unless major.minor.patch of the version
 * string returned by IPD_QUERY_NODE equal its own (ifcs_get_version()); the
 * default is what the vendor ipd.ko shipped with that SDK reported. */
static char *ipd_legacy_version = "0.12.41.1.0.1";
module_param_named(legacy_version, ipd_legacy_version, charp, 0444);
MODULE_PARM_DESC(legacy_version, "driver version reported to legacy_abi userspace");
...
-        strscpy_pad(q.version, ipd_version, VERSION_SIZE);
+        strscpy_pad(q.version, ipd_legacy_version, VERSION_SIZE);
```

Only the legacy QUERY_NODE reply uses it. The module banner, sysfs and
`modinfo` still say 0.16.2.0.0.0. It is a parameter so another SDK drop with a
different libifcs version does not need a rebuild
(`legacy_version=0.12.NN...`). No change to `kernel.nix`, no new patch file,
nothing in the vendor userland was modified.

Build: `nix build --impure -f .../scratchpad/ipd.nix` ->
`/nix/store/q6y9126nya7mxly10kzx1ivbpxq6n446-ipd-aarch64-unknown-linux-gnu-0.16.2-7.3.3`
(`lib/modules/7.3.3/updates/ipd.ko.xz`, sha256 `ff23a187...84e4f7`).

## What happens after the fix (H)

strace of the python shell's attach (`ipd-legacy-strace-after-fix.txt`,
RUPT_WAIT lines removed):

```
openat("/dev/ipd", O_RDWR) = 7
ioctl(7, 'I' nr 2)  = 0                                       # QUERY_NODE
mmap(NULL, 131072, RW, MAP_SHARED, 7, 0xc0200000)              # BAR0, as stock
mmap(NULL, 8192,   RW, MAP_SHARED, 7, 0xae2d8000)              # pool (pool_baddr)
ioctl(7, nr 0xa) = -1 EINVAL                                   # HRR_FREE before alloc
ioctl(7, nr 0x9) = 0 ; mmap(NULL, 8320, ..., 0xae2dc000)       # HRR_ALLOC
ioctl(7, nr 0x11) = 0 ; mmap(NULL, 4096, ..., 0x109c85000)     # RING_PAGE_ALLOC x256, pages > 4 GiB
...
```

Mapping sizes over both opens: 2 x 0x20000, 2 x 0x2000, 2 x 8320 (0x3000
rounded), 6 x 0x4000, 2 x 0x48000, 512 x 0x1000 — the same set the stock
`/proc/PID/maps` shows. ioctls used: nr 1, 2, 3, 4, 7, 8, 9, 0xa, 0xb, 0xf,
0x11, 0x13, 0x17, 0x1a; all return 0 except `HRR_FREE` (once per open) and
`RING_FREE` (8 per open), which return `-EINVAL` when issued before the
matching alloc. Userspace ignores those. Whether the old module also returned
an error there is not known (no stock strace exists).

The library does its own CPU-block init (`ii_is_cpu_block_init_done`,
`ii_cpu_block_init_scratchpad_set` are called from `_ii_pcie_attach`, V), so
skipping the kernel-side `inno_cpu_block_init()` in legacy mode was right.

Final run, no strace, driver loglevel 2
(`evidence/mainline-sdk-2026-11-10/ipd-legacy-node-create-run.txt` and
`...-state.txt`):

- SDK log: `Node 0 created and enabled.`, no `devport setAttr failed`, then
  `Knet config Start` ... `Change knet interface name` / `Knet config done` /
  `misc config end.`; script exit code 0.
- `ls /sys/class/net`: `enp0s1 enp0s3 fpp1 ... fpp32 inno_0_0129 inno_0_1000 lo`.
  `fppN`: MTU 9462, MAC `0e:48:c6:7a:55:cf` (fpp1) ... `:ee` (fpp32),
  `NO-CARRIER,UP` (no PHY bring-up, no optics). `inno_0_1000` is UP with
  carrier.
- dmesg: `Innovium Switch PCIe Driver opened successfully` (python shell),
  closed, opened again (`innovium.user`), then
  `ipd 0001:01:00.0 fpp1: renamed from inno_0_0001` ... `fpp32: renamed from
  inno_0_0032`. No oops, no SError.
- `/proc/interrupts`: MSI-X vectors 40-47 `inno_irq-0-napi` (33),
  `1-async` (0), `2-tx` (0), `3-rx` (0), `4-err` (0), `5-lrn` (0),
  `6-pic` (9905 after ~70 s and counting), `7-rx-mcu0` (0). Same picture as
  stock (only `pic` fires when idle) plus the extra eighth vector.
- `innovium.user` pid 20700, 15 threads, alive when I stopped.

## Still failing / not verified

1. **CPU RX path rejects frames** (H, cause I). With the SDK up, dmesg logs
   about once a second:
   ```
   IPD-ERR:inno_unpack_ldh_header_v0:71: Unpack-Unsupported VF 3   (or VF 7)
   IPD-ERR:inno_rx_ring_poll:1277: Invalid LDH
   ```
   31 of them in ~70 s (16 x VF 3, 15 x VF 7), while `inno_0_1000` shows
   TX 29 packets / RX 0: the host autoconfigures that netdev
   (169.254.171.170/16, fe80::/64) and each frame it sends appears to come
   back on a CPU RX ring with a header whose `vf_type` the 0.16.2 v0 parser
   does not accept (`inno_ldh_v0.c` handles only VF 0). The `napi` vector
   count (33) matches. The stock dmesg has no such lines, but stock ONIE
   probably sent nothing on these interfaces, so that is not a comparison.
   Candidates, none tested: the TX header the driver builds from the
   legacy-translated `syshdr` blobs is wrong (SET_NETDEV conversion, 12/24 ->
   20/40 byte slots); the RX header is read at the wrong offset
   (`DMA_GLOBAL` cacheline setting / `cache_align` 64 vs the old 128, see the
   source-compat review section 4); or the old module accepted these VF
   types. This is the first thing to look at before any traffic test. It is
   noisy at loglevel 2 but otherwise harmless so far.
2. Not done, by instruction: Credo PHY bring-up alongside the SDK, link,
   traffic.
3. `HRR_FREE` / `RING_FREE` returning `-EINVAL` before the first alloc: see
   above; ignored by userspace.
4. The full system closure was not rebuilt; only the module was built and
   insmod'ed. After the next `nixos-rebuild`, `modprobe ipd` picks up the
   same patch.

## Operational notes

- The first successful run died at the moment `timeout -s KILL` killed the
  strace that was tracing it (I: strace took its tracees with it; all `RUPT_WAIT`s returned at once and the
  driver cleaned up normally). Do not conclude a crash from that.
- Driver loglevel 7 during a successful start overflows the kernel log buffer
  within seconds (two lines per `RUPT_WAIT`, ~140/s under strace). Use it only
  for the first seconds of an attach.
- `auto_load_user.sh -d` leaves `innovium.user` holding the chroot's private
  mount namespace; `pkill innovium.user` releases everything and the driver's
  close path removes the netdevs and MSI-X vectors cleanly (seen four times).

## State left on the switch

- Loaded module: the fixed build above, `insmod`ed from
  `/root/ipd-test.ko.xz` with `legacy_abi=1` (`legacy_version` default
  `0.12.41.1.0.1`), loglevel 2. The file itself was removed afterwards; the
  module stays loaded until `rmmod`/reboot.
- `innovium.user` running (started by the vendor script, daemon mode, 32x400
  knet), netdevs present.
- My files in `/root` removed. `credo.log`, `innovium.log`, `run.log`,
  `strace.out`, `strace.txt` were there before and are untouched.
- No reboot, no register access outside the driver/SDK, nothing under
  `nepeat/nix` changed except `ipd-legacy-abi.patch`. Nothing committed.

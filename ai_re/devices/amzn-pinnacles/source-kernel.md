# Exact Amazon MT8183 kernel source recovered

## Result

Amazon's official Fire HD 10 (11th generation) GPL archive contains an exact
**Linux 4.4.146** MT8183 kernel tree, not the 4.4.302-only GitHub mirror that
blocked the earlier module plan.

Archive:

```text
Fire_HD10-7.3.2.6-20221121.tar.bz2
https://fireos-tablet-src.s3.amazonaws.com/fX8Oef0HfwEDOoUX39u0ks9QhY/Fire_HD10-7.3.2.6-20221121.tar.bz2
size: 589727649 bytes
ETag/MD5: 8f3f1baa388919a9abff3982b85ae930
```

The outer archive contains `platform.tar`, Amazon build scripts and this build
configuration:

```text
KERNEL_SUBPATH=kernel/mediatek/mt8183/4.4
DEFCONFIG_NAME=trona_defconfig
TARGET_ARCH=arm64
```

The recovered kernel `Makefile` says:

```text
VERSION = 4
PATCHLEVEL = 4
SUBLEVEL = 146
```

That matches the live yacht kernel (`Linux 4.4.146+`). The live
`/proc/config.gz` was copied and successfully accepted by this tree through
`olddefconfig`, `prepare` and `modules_prepare`.

## Important correction: the MASP symbol is a dummy

The source settles an ambiguity from the earlier kallsyms-only analysis.
`masp_hal_set_dm_verity_error()` is not a useful verity switch:

```c
int masp_hal_set_dm_verity_error(void)
{
        int ret = 0;
        /* do nothing, used when platform security porting is not completed */
        return ret;
}
```

So merely calling that symbol cannot disable dm-verity. The prior notes that
describe it as a possible control point are superseded by this source audit.

## What the source opens

The same kernel exports GPL `kallsyms_lookup_name`, and module signatures are
disabled on the live device. Once a minimal module passes the live
`CONFIG_MODVERSIONS` ABI check, it can resolve non-exported kernel symbols by
name. Two practical paths then exist without forging Amazon's RSA signature:

1. **Second-stage ROM:** keep signed stock boot/system intact, obtain per-boot
   root, then mount a custom userspace stored under `/data` using a kernel
   module plus bind/pivot orchestration. This avoids poisoning the verified
   stock system partition.
2. **Warm boot:** implement the missing arm64 kexec transition in a module.
   The stock config has `CONFIG_KEXEC` disabled, but the transition can be
   carried as module code and private symbols resolved through
   `kallsyms_lookup_name`. This is higher risk and should be attempted only
   after a harmless module ABI probe succeeds.

`kernel-modules/hello/` is the non-destructive first probe. Do not build a
verity patcher or warm-boot module until its `module_layout` and symbol CRCs
are confirmed against the running kernel.

## Custom kernel code executed on yacht

This is no longer only a source-tree lead. On 2026-08-25 a fresh root window
was used to copy yacht's six protected shipping modules from
`/vendor/lib/modules`. Their embedded version records establish the exact live
ABI:

```text
vermagic:             4.4.146+ SMP preempt mod_unload modversions aarch64
module_layout CRC:    0x62fa6c4c
printk CRC:           0x985558a1
kallsyms_lookup_name: 0xe007de41
```

The retail PS7326 OTA is not ABI-identical: its `module_layout` CRC is
`0x0d282b17`. Do not use that value on yacht. The other two CRCs above happen
to match retail, but yacht's own modules are the authority.

Using yacht's authentic `module_layout` and `printk` records, the hello probe
was linked on `erin@10g.warc.zip`. Its `__versions` section contains exactly:

```text
0x62fa6c4c module_layout
0x985558a1 printk
```

Live result:

```text
insmod_rc=0
yacht_hello 16384 0 - Live ... (O)
yacht_hello: exact 4.4.146 module loaded
rmmod_rc=0
yacht_hello: unloaded
```

This proves arbitrary GPL-compatible module code can execute in the stock
kernel once transient root is obtained. Module signatures are not the barrier;
the exact `CONFIG_MODVERSIONS` records were. A root-only `/proc/kallsyms` dump
was also preserved (52,887 names; addresses remain zeroed by `kptr_restrict`).
That is still sufficient for an in-kernel module to resolve private symbols
through exported `kallsyms_lookup_name()`.

The read-only `kernel-modules/introspect/` follow-up proved that resolution
path live, then unloaded cleanly:

```text
machine_shutdown=ffffff8008085d5c
secondary_holding_pen=ffffff8008081e60
idme_get_dev_flags_value=ffffff8008a3dc48
idme_get_item=ffffff8008a3d588
force_ro_store=ffffff80089d38b8
sys_kexec_load=ffffff80080c9c28
sys_kexec_file_load=ffffff80080c9c28
```

The two kexec syscall names resolving to the same address strongly indicates
they are weak unsupported-syscall stubs, not a hidden working kexec
implementation. `machine_shutdown` and the arm64 holding pen are real and
distinct, so a module-carried transition remains the relevant design.

The Amazon 4.4 tree contains no arm64 `machine_kexec.c` or
`relocate_kernel.S`; only the ordinary shutdown preparation in `process.c`.
So the transition cannot simply be compiled from a dormant file in this
archive. The closest primary-source donors are Android's arm64 kexec ports in
the Trusty 4.14/common and older Qualcomm downstream trees. Port only the
small relocation/MMU-off path to yacht's 4.4 APIs; do not import a modern
`struct kimage` implementation wholesale.

## Current build state

The exact source and live config prepare successfully on macOS using an arm64
Linux cross-compiler. Old 4.4 host utilities need compatibility handling:

- build host tools as x86_64 (`-arch x86_64`) so old modpost's Mach-O section
  trick links under Rosetta;
- use `-fcommon` for the bundled old dtc;
- provide a host `elf.h` (macOS has none);
- skip host `extract-cert` for external-module preparation; target config is
  unchanged and module signing is disabled.

The live module-version proof is complete. Amazon's release is nevertheless a
partial tree: many enabled directories contain Kconfig files but omit their
Makefiles and sources. A full vmlinux build therefore fails during
`modules.builtin`; do not treat this as a compiler mismatch. For external
modules, provide the two omitted linker scripts (`scripts/module-common.lds`
and `arch/arm64/kernel/module.lds`), build host `modpost` with `-fcommon`, set
`CONFIG_LOCALVERSION="+"`, and seed `out/Module.symvers` only with CRCs taken
from yacht's own shipping modules.

## Remote Linux builder

The user-provided build host is reachable as `erin@10g.warc.zip`:

```text
Linux 7.1.3+deb14-amd64, x86_64
Docker available at /usr/bin/docker
about 32 GiB free on /home
```

No host `clang` or `aarch64-linux-gnu-gcc` is installed, so builds run in a
Debian container. GCC 12 successfully built and linked the harmless external
module; the exact Android `r316199` clang/GCC 4.9 pair remains preferable for a
full kernel or sensitive arm64 transition code. Remote workspace:
`/home/erin/ai_re-yacht-build`.

The read-only introspection target is complete and its expanded prerequisite
probe is built. The staged implementation plan, including non-jumping dry-run
gates and the separately authorized `machine_shutdown`/MMU-off milestone, is
documented in [warm-boot.md](warm-boot.md).

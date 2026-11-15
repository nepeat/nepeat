# Broadcom SAI/SDK on big-endian PowerPC — research, 2026-11-14

Web research by a Sonnet sub-agent; "verified" means it read the page or repo,
"inferred" is its reasoning. Nothing here was built or run by us.

No Broadcom SAI or SDK binary for PowerPC exists in public. The most promising route is to build the SAI-independent layers from source for big-endian ppc64 and write or adapt a thin SAI on top. Nothing I found is a ready libsai for this combination.

"Verified" below means I read it in the repo or page. "Inferred" is my reasoning.

## Ranked routes

**1. OpenBCM 6.5.27 built for BE PowerPC, with a custom thin SAI/syncd layer on top.**
- Verified: the OpenBCM repo is https://github.com/Broadcom-Network-Switching-Software/OpenBCM (default branch master). Its README lists Trident (TD2/3/4), Tomahawk (TH/TH2/TH3) and DNX.
- Verified: `sdk-6.5.27/systems/linux/user/` contains `gto`, `gto-2_6`, `keystone-2_6`, `keystone_le-2_6`, `iproc_be-3_6`, `slk_be`, `wrx-3_7`, `x86-smp_generic_64-2_6` and others.
- PowerPC target: only `gto` and `gto-2_6` (the "GTO" name refers to the MPC8548 e500v2 board). `make/Makefile.linux-gto` sets `-DSYS_BE_PIO=1 -DSYS_BE_PACKET=0 -DSYS_BE_OTHER=1`, `BE_HOST=1`, `ARCH=powerpc` and `-m32`. The compiler is `powerpc-broadcom-linux-gnuspe`, which is e500v2 SPE, not e6500. `gto-2_6` uses WindRiver `ppc_e500v2` toolchains.
- Not PowerPC: `wrx` is MIPS (XLP), `keystone` is ARM, `iproc_be` and `slk_be` are ARM big-endian.
- Inferred: the BE_HOST and SYS_BE flags show the core SDK is endian-aware. Tomahawk support in a PowerPC build is unconfirmed, and I found no report of any 6.5.x build on a T2080 or other e6500 host.
- Required: a new `linux-ppc64be` platform makefile, either 64-bit or `-m32` under compat. It needs the BDE/KNET kernel modules rebuilt for Linux 7.x; those are GPLv2 and sit in `gpl-module` and `dcb-gpl-modules` inside OpenBCM. It also needs a Tomahawk BE audit and the BDE PIO/DMA byte-swap settings for the T2080 PCIe controller.
- No SAI exists on top of it, so you'd write one. An open reference is `sonic-sairedis`/`saivs` plus the SAI headers, but I did not find a source SAI for Tomahawk.
- Licence (verified): the Broadcom Switch Software License at `Legal/LICENSE`. It grants a royalty-free, perpetual right to use, modify and distribute source and derivatives, with sublicensing allowed. It forbids reverse engineering of object-code-only parts. Kernel modules are GPLv2. Fine for personal use.

**2. OpenMDK, a much smaller driver, plus a custom control plane.**
- Verified: https://github.com/Broadcom-Network-Switching-Software/OpenMDK has `bmd/PKG/chip/bcm56960_a0` (Tomahawk, with attach, init, port_mode_set, mac_addr_add, etc.) and `bcm56850_a0`. It also has `cdk`, `phy` and `libbde` directories. The last push was 2025-08-15.
- Verified (page content, not tested): the EdgeNOS repo (https://github.com/wrightca1/edgenos, MIT) claims to have built OpenMDK SDK libs plus an "edged" datapath daemon for PowerPC, on the AS5610-52X (BCM56846, P2020 e500, kernel 5.10). Its README says "datapath up, OSPF running". Its own licence notes say the Broadcom SDK terms are source-available, not GPL-compatible. I did not verify that it works or inspect the daemon's code.
- Required: write a SAI-like shim (L2/L3/ACL/port) over the BMD API, or port the ideas from EdgeNOS's `core/datapath` and `asic/bcm56846`. This is the lowest-risk way to get a PowerPC BE binary driving a Broadcom ASIC. I did not check whether OpenMDK has a BCM56930 or the b930 ID specifically; only 56960_a0 appeared in the tree. The feature set is far smaller than SAI.
- Licence: not read directly; the repo has a `Legal` directory.

**3. Run the amd64 libsaibcm and syncd under qemu-user plus the real BDE/KNET modules.**
- Verified: SONiC's packages are amd64 only (`libsaibcm_15.2.1.0.0.0.50.1_amd64.deb` in `platform/broadcom/sai-xgs.mk`; `opennsl-modules` control file says `Architecture: amd64`).
- Required: patch qemu-user to translate the BDE ioctls (`LUBDE_*` structs) and the KNET ioctls. These custom ioctls are "Unsupported" in stock qemu-user, and it needs per-ioctl thunk definitions in `linux-user/syscall.c`. See https://carlo.marag.no/posts/add-custom-ioctls-qemu/ and https://gitlab.com/qemu-project/qemu/-/issues/3185 (verified: a wrong ioctl arg type breaks only big-endian hosts).
- Inferred, unverified: for 32-bit MMIO the TCG byte-swap on a LE guest load may cancel the PCIe controller's swap, so raw register reads come out right. DMA descriptors written by the LE guest would be seen as raw LE by the device. This needs testing, and 64-bit and unaligned MMIO accesses are risky.
- Other unknowns: mmap of `/dev/linux-kernel-bde` for DMA memory from an emulated address space, performance, and the legacy-TH SAI needing the matching SDK modules. The saibcm-modules kernel side would also have to be built BE.
- Same-endian reports: I found none for libsaibcm under qemu-user (not even on arm64). SONiC's arm64 Broadcom builds use native iproc and gts SDKs, not emulation.
- Licence: the Broadcom SAI binaries are redistributable via SONiC's build URL, but their terms aren't public. Treat as unknown.

**4. SONiC `saivs` (libsaivs) driving real netdevs, with KNET or a software datapath.**
- Not tested and I found no source. This gives SONiC behaviour (orchagent, CLI) but no ASIC programming, so no hardware line-rate forwarding unless you add a Linux-kernel-to-ASIC offload layer yourself. Cheap to try; it is a stopgap.

**5. Source-available SAI to adapt: none found.**
- Verified: https://github.com/BrcmSAI/SAI ("SAI 3.7", spec 1.5.2, TD2/TD3/TH/TH2/TH3) ships only `bin/x86-xgs5-deb80` (`libsai.so.1.0.gz`, KNET and BDE `.ko`), last pushed 2020-08-13. The repo is binary only, and the "Dependencies: None" claim shows no source.
- Verified: https://github.com/opennetworklinux/OpenNSL (6.4.6.x) has `bin/wedge-trident` and `bin/wedge100` x86 binaries only.
- Verified (search result, not opened): Dell/OPX used Broadcom SAI in binary form (OPX porting guide, https://github.com/open-switch/opx-docs/wiki/OPX-porting-guide). The OpenSwitch mailing-list thread says the community SAI repo is not the full source: https://lists.openswitch.net/g/dev/topic/question_about_sai_bcm_esw1/23198777
- OpenNSL tool suite https://github.com/Broadcom-Network-Switching-Software/OpenNSL-Tool-Suite was archived 2026-01-14 (verified via search).
- Unknowns: Stratum's bcm wrapper and SDKLT were not verified. A search says SDKLT targets BCM56960 Tomahawk, with only the logical-table API definitions under Apache 2.0 and the rest under the Broadcom Switch Software License. I could not retrieve the SDKLT repo to check big-endian or PowerPC support.

## saibcm-modules (item 4 of your brief)
Verified by cloning branch `sdk-6.5.35-xgs` (https://github.com/sonic-net/saibcm-modules):
- `systems/linux/user` has only common, gts, iproc (3_14/4_4/64), slk, x86 (5_10, 6_10, smp_generic_64-2_6, 64-fc28) and xlr. There is no gto or PowerPC target, and nothing in `make/` for it.
- The only `BE_HOST` reference is one `#if defined(BCM_CMICM_SUPPORT) && defined(BE_HOST)` in `linux-kernel-bde.c` (line 827). `make/Make.config` has no ENDIAN settings.
- So that branch lost the PowerPC support the full OpenBCM tree keeps. Branches `sdk-6.5.32-xgs-legacy-th` and `sdk-6.5.37-xgs` also exist; I did not inspect their trees. SONiC builds the legacy-TH modules from this repo as `opennsl-modules-legacy-th` (amd64).
- Practical consequence: take the kernel modules from OpenBCM `sdk-6.5.27` (which has `gto`), not from saibcm-modules, when targeting BE. Your 6.5.27 sources already dispatch 14e4:b930 to the TH driver.

## Historical NOS on PowerPC plus Broadcom
- Verified (search results): ONL lists the AS5610-52X (P2020, BCM56846) and AS6700-32X (P2041, BCM56850) as PowerPC platforms; the ONL installer makefile has a `powerpc` target. OF-DPA ships `bin/as6700-trident2-fsl14` (https://github.com/Broadcom-Switch/of-dpa). OF-DPA/Indigo is a different API from SAI.
- I found no evidence of any NOS shipping SAI, SONiC, OPX, ONL or ICOS for PowerPC plus Tomahawk. Tomahawk boxes in this list (AS7710/7712, S6000-ON) are x86, and the OPX SAI binary covered Trident-II, Maverick and Tomahawk on x86 only.

## Gaps
- I could not check OpenBCM's Tomahawk code for BE-host assumptions, which a build and test would show.
- I did not read the OpenMDK licence text.
- I did not verify that EdgeNOS actually works.
- I found no source for Stratum's bcm layer.

# AS7760-32X as a native-SONiC target — onboarding answers

Project notes: [../sonic-nixos/PROGRESS.md](../sonic-nixos/PROGRESS.md).

1. **ASIC / switchdev?** Broadcom BCM56930 (PCI `14e4:b930`, two functions);
   OpenBCM 6.5.27 drives it through the Tomahawk code. No switchdev driver.
2. **SAI family?** Tomahawk-class XGS ("legacy-TH" in SONiC's packaging). The
   published libsai for it is amd64 only.
3. **Control-plane CPU?** NXP T2080, ppc64 **big-endian**. No Broadcom SAI or
   SDK binary exists for PowerPC ([research](../sonic-nixos/research-ppc-sai-20261114.md)).
   As the plan is written this is its kill criterion for the ASIC phase; the
   stack itself (virtual SAI) is not affected and is what runs here first.
   Routes for the ASIC, most promising first: OpenBCM 6.5.27 built for BE ppc64
   plus a thin SAI of our own; OpenMDK (has `bcm56960_a0`) plus a smaller shim;
   amd64 libsaibcm under qemu-user (cross-endian here, unlike the CS8210, and
   needs custom ioctl translation; last resort).
4. **Upstream device dir?** None for this board. The x86 sibling
   `x86_64-accton_as7712_32x-r0` exists upstream and is the reference.

Kernel side: saibcm-modules sdk-6.5.35 compiled against Linux 7.3.4 on x86
([sdk-research](sdk-research/README.md)), but that branch dropped the
PowerPC/BE build support that OpenBCM 6.5.27 still has (`gto` targets, 32-bit
e500 toolchain assumptions). Fans are already managed (`thermal-as7760-fan`).

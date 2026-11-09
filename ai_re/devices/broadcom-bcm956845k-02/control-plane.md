# Control-plane reconnaissance — 2026-11-08

Evidence: live serial capture [serial-20261108-183606.txt](serial-20261108-183606.txt).
Initial inspection used an already booted device. Subsequent cold-boot capture
and firmware acquisition are recorded in [firmware-dump.md](firmware-dump.md)
and [boot-20261108-184531.txt](boot-20261108-184531.txt).
The stored Linux image has since booted to a root shell; see [linux-shell.md](linux-shell.md).

## Inventory

| Component | Observed result | Interpretation / limit |
| --- | --- | --- |
| Board | `sysModel()` → `Broadcom SBC8548` | Runtime BSP identity |
| CFE board | `BCM98548XMC`, board ID `0x51` | Later cold-boot report |
| Platform | SDK `version` → `GTO_MPC8548` | Linux reports BCM98548 XMC, SVR `0x80310021` |
| CPU architecture | Linux `/proc/cpuinfo`: e500v2 | Rev 2.2, PVR `0x80210022`, one processor listed |
| BSP | `sysBspRev()` → `2.0/2` | Runtime BSP revision |
| OS | VxWorks 6.5, WIND 2.10 | Native `version`; kernel built Dec 16 2010 19:00:10 |
| SDK | `sdk-xgs-robo-5.9.1` | Build Dec 16 2010 18:59:47 |
| Physical memory top | `sysPhysMemTop()` → `0x20000000` | Consistent with 512 MiB RAM starting at zero; address report, not a memory test |
| VxWorks memory top | `sysMemTop()` → `0x20000000` | Same as physical memory top |
| Heap free | 373,070,352 bytes (~355.8 MiB) | Snapshot, not total installed RAM |
| Heap allocated | 109,524,208 bytes (~104.5 MiB) | Snapshot |
| Clock getter | `sysClkFreqGet()` → 400,000,000 | Domain not yet confirmed; do not label this CPU frequency |
| Timer clock variable | `sysTimerClkFreq` → 50,000,000 | Runtime global, not CPU core frequency |
| Core frequency variable | `coreFreq` → 0 | Uninitialized/unpopulated value; provides no CPU clock evidence |
| Actual core clock | CFE reports 997 MHz CORE / 998 MHz CPU | Approximately 1 GHz; CPU type `0x80210022` |
| Linux core clock | `/proc/cpuinfo`: 997.500000 MHz | Timebase 49,875,000 Hz |
| Actual CCB clock | CFE reports 399 MHz | Consistent with 400 MHz BSP clock getter, distinct from CPU core |
| Flash filesystem backing size | `flashFsSize` → 133,169,152 (`0x07f00000`) | 127 MiB; physical chip total may be larger |
| Flash filesystem | FAT16, VFAT names, 512-byte sectors, 4 sectors/cluster | `dosFsShow "flash:", 0` |
| Flash free | 115,415,040 bytes (~110.1 MiB) | Filesystem report |

NXP reference: [MPC8548E reference manual](https://www.nxp.com/docs/en/reference-manual/MPC8548ERM.pdf)
identifies the e500v2 core; [product page](https://www.nxp.com/products/MPC8548E)
lists family capabilities including 32 KiB instruction + 32 KiB data L1,
512 KiB L2 and DDR/DDR2 support. These are family specifications, not
measurements of this board's cache enablement, RAM type or clock settings.

## Flash directory

Read with `ls "flash:"` and `ll "flash:"` in the native shell:

| File | Bytes |
| --- | ---: |
| `bcm.gto.gz` | 5,943,148 |
| `rc.soc` | 62,104 |
| `config.bcm` | 1,265 |
| `tdsanity.soc` | 57,417 |
| `gsanity.soc` | 5,392 |
| `uImage.gto-2_6` | 11,401,844 |

All listing timestamps were Feb 2 2017; these are filesystem metadata, not
proof of build dates. The running boot line names `bcm.gto.gz` on `flash`.
`uImage.gto-2_6` is a promising Linux-image lead based on its name only.
All six files were subsequently extracted and hashed. The Linux image header
and data CRCs passed; its first component contains Linux
`2.6.21.7-hrt1-WR2.0bl_standard`. Neither Linux nor an alternate boot image
was booted during the initial reconnaissance; Linux was subsequently booted
successfully to a root shell as recorded in [linux-shell.md](linux-shell.md).

`dosFsShow` also reports read-access timestamp updates disabled and no
automated disk check on mount. No check/repair or synchronization was invoked.

## Console workflow and queries

Attach: `tmux attach -t serial-bcm956845`; detach: Ctrl-b, then d.
Confirmed 9600 8N1, no flow control. Device remains powered on.

SDK `BCM.0>` prompt:

```text
help
version
help shell
shell
```

Native `->` prompt accepts C-like function calls and symbol expressions:

```text
printf "%s\n", sysModel()
printf "%s\n", sysBspRev()
version
memShow
sysPhysMemTop
sysMemTop
devs
ls "flash:"
ll "flash:"
lkup "Clk"
lkup "Freq"
lkup "flashFs"
coreFreq
sysClkFreqGet
sysTimerClkFreq
flashFsSize
dosFsShow "flash:", 0
i
```

`sysModel` alone returns a pointer; `printf` renders the pointed-to model
string. The attempted regex-like `lkup "[Ff]req"`, `lkup "sys.*[Cc]lk"`
and `lkup "sys.*[Cc]pu"` returned no symbols. Plain substring lookups
(`"Clk"`, `"Freq"`, `"Cpu"`, `"clock"`, `"flashFs"`) worked.
Native `help` pages output; `q` + Enter stopped its pager successfully.
Broad `lkup "sys"` also paged after a screen of symbols; stopped with `q`.

`devs` listed `/null`, `/tyCo/0`, an AIO pipe, `vaal:`, `flash:`, and
`/pty/telnet0.S` / `/pty/telnet0.M`. The PTY names alone do not prove a
network-reachable telnet listener. Task listing includes SDK and OS tasks.
Specifically observed FTP (`ipftps`), NFS (`tNfsd*`, `tMountd`, `tPortmapd`)
and telnet (`tTnetrBCMd`) tasks. Their presence is a transfer-path lead;
network reachability and authentication have not been tested.

SDK `version` prints a long **compiled-in support list** of ASICs and PHYs.
It is not an inventory of installed chips. The boot line contains network
boot credentials; avoid copying it into public reports.

## Next steps

- Completed in follow-up: CPU/CCB clocks from CFE, mapped flash geometry,
  full acquisitions, filesystem extraction and image CRC validation.
- Establish exact CPU SoC silicon suffix/revision and physical flash chip identity.
- Inspect `rc.soc` / `config.bcm` as text, without sourcing either script.
- Return to SDK shell using its documented native-shell return mechanism,
  then read `soc` and port status to identify the fitted ASIC/40G configuration.
- Firmware/configuration backups now exist; establish a recovery and temporary
  boot path before attempting an alternate OS.

## Linux / NOS feasibility assessment

Initial assessment: Linux is plausible, and a small custom Linux + ASIC SDK
control plane is a credible research target. An ordinary current Debian or
SONiC installation is not a demonstrated option for this hardware.

- **Boot/kernel:** the stored `uImage.gto-2_6` has now been extracted and its
  CRCs validated. Its kernel is Linux 2.6.21.7-hrt1-WR2.0bl_standard. This is
  now boot-tested: a Linux root shell works, management Ethernet controllers
  and CFI flash devices probe, and the root filesystem is an initramfs.
  Network operation and ASIC integration under Linux still need checking.
- **Userspace ABI:** the likely e500v2 CPU lacks the classic PowerPC FPU.
  Debian's historical [PowerPCSPE port](https://wiki.debian.org/PowerPCSPEPort)
  explicitly listed MPC8548. That wiki is stale as a current installation guide:
  [Debian's glibc changes in 2019](https://lists.debian.org/debian-glibc/2019/09/msg00022.html)
  dropped powerpcspe, and a [2021 Debian bug closure](https://bugs.debian.org/694425)
  confirms it no longer existed in Ports. An archived userspace or custom
  soft-float build is more plausible than a current stock Debian image.
  FPU emulation would require separate kernel/ABI validation.
- **ASIC programming:** Linux booting does not establish front-panel
  forwarding, VLAN/L3 programming, packet DMA or link/PHY support.
  Broadcom's public [OpenBCM repository](https://github.com/Broadcom-Network-Switching-Software/OpenBCM)
  contains APIs/drivers, including [original Trident source](https://github.com/Broadcom-Network-Switching-Software/OpenBCM/blob/master/sdk-6.5.27/src/soc/esw/trident.c).
  This is a useful lead, not proof of a complete build for this board or its
  exact chip. Its core is under a [Broadcom-specific license](https://github.com/Broadcom-Network-Switching-Software/OpenBCM/blob/master/Legal/LICENSE),
  rather than a uniformly GPL/BSD/Apache stack.
- **Full NOS:** current [SONiC build machinery](https://github.com/sonic-net/sonic-buildimage/blob/master/slave.mk)
  has AMD64/ARM architecture paths. This PowerPC target would require additional
  architecture/platform work, a suitable ASIC integration (e.g. SAI) and
  substantially more storage than the current flash offers for a typical
  containerized deployment. With 512 MiB RAM, a deliberately small control plane
  is a more realistic starting point. This is an engineering assessment, not
  an attempted SONiC build result.

Suggested milestones: preserve firmware → extract/identify existing Linux
image → obtain shell in a temporary Linux boot with a recovery path → verify
management Ethernet/storage → prove SDK access to ASIC and packet I/O → add
VLAN/L3 configuration and routing-daemon integration. The existing Linux image
has now been booted temporarily; no persistent boot-configuration change was made.

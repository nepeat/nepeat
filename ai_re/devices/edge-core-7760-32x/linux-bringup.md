# linux 7 bring-up — 2026-11-14

The workstation builds big-endian e6500 ELFv2 Linux 7.3.4 and NixOS from the
pinned unstable flake. The successful hardware boots so far use the separate
static musl/BusyBox RAM root, with selected Nix-built diagnostic tools. The
full NixOS SD root has not been installed or booted. All console interaction
uses `serial-7760` tmux/tio, 115200 8N1, with captures in this directory.

## changes required by the hardware

- Normal `console=ttyS0,115200n8` works. `earlycon` and fixed-UART early-debug
  attempted early mappings before Book E memory setup and prevented boot.
  Temporary UART tracing patches isolated the fault; none ship in the kernel.
- DPAA needs reserved BMan FBPR (16 MiB), QMan FQD (4 MiB) and PFDR (32 MiB)
  pools, modern two-cell QorIQ clock references, v3 hardware port IDs,
  `fsl,fman-ports`, channel ranges and the management MAC's internal SGMII PCS.
  These follow the kernel's T2080/T2081 and QorIQ DTS definitions; the board's
  original external BCM54616S PHY at MDIO address 31 is preserved.
- Keep the old receive-port compatible strings as secondary entries.
  [U-Boot 2014.07 LIODN macros](https://raw.githubusercontent.com/u-boot/u-boot/v2014.07/arch/powerpc/include/asm/fsl_liodn.h)
  use those strings when injecting DMA identifiers. Modern names alone omit
  the RX LIODN. Live readback after restoration is 0x425 (973 + 88).
- The old PCIe4 CPU window at 0xd8000000 overlaps the 16-GiB RAM map. Move its
  128-MiB CPU aperture out of RAM and correct the nested bridge range
  (first to 0xc60000000, which had no local access window and machine-checked
  on the first MMIO read; since 2026-11-14 0xc40000000, where U-Boot opens it);
  retain PCI bus addresses 0xd8000000–0xdfffffff. Both BCM56930 functions have
  32-KiB BAR0 and 8-MiB BAR2 assigned afterward.
- `pcie_aspm.policy=performance` prevents intermittent PCI config failures.
  The FSL host driver's link check requires L0; firmware-enabled L0s correlated
  with failed reads, bad header values and missing IRQ assignment. Live policy
  change restored reads; a cold boot with the policy assigned IRQs. No ASIC
  driver, DMA or packet-forwarding test has been performed. VPD is unreadable.
- NOR reports manufacturer 0x89, chip 0x227e, **CFI command set 0002**. Enable
  `MTD_CFI_AMDSTD`; Intel-only support detected the chip but rejected its command
  set. All eight original partitions now enumerate. The vendor `byteswap`
  property is not needed for the successful CFI probe; no byte-order change
  was made.
- **Correction 2026-11-14:** the paragraph below is superseded. Register
  reads show `ethtool -K rxhash` never changes the FMan path (KeyGen always
  on), and full NixOS works with hashing on, incl. cold-boot DHCP and 256 MiB
  bulk RX with zero errors. Stack rx_dropped is VLAN-tagged multicast from
  the upstream trunk (no 8021Q in the kernel). See PROGRESS.md.
- (superseded) Hardware RX hashing blocks received frames in the FMan processing path.
  The MAC receives frames and TX confirmations complete, but network RX stays
  zero. `ethtool -K eth0 rxhash off` restores DHCP and ping through the supported
  driver feature toggle. Preserve this startup workaround in both RAM init
  and the NixOS networkd pre-start hook. The underlying KeyGen/hash problem is
  not yet diagnosed. Full-memory (16-GiB) DHCP, pings and a 4,922,999-byte TFTP
  transfer subsequently passed; the transferred kernel SHA256 matched. A 1920-MiB memory-limit trial also failed before disabling
  hashing, so the failure is not solely high DMA addresses.

## measured hardware checks

- All eight CPU threads and 16 GiB RAM enumerate in unrestricted boots.
- Original SD SA08G is 7,948,206,080 bytes. No persistent filesystem mounted.
- Read hashes of MTD0, MTD1, MTD5 and MTD7 match the verified acquisition;
  SD sector zero also matches. Other partitions were not re-hashed in this test.
- Two EHCI hosts and USB hub 0424:2514 enumerate; no USB disk is attached.
- Four GPIO character devices enumerate. The captured DT has no SPI controller
  node; the factory GPIO-backed SPI hookup still needs recovery before use.
- Fan module loads without forced symbol/version overrides. Twelve rotors
  report about 18–22k RPM; four LM75 sensors report about 24.5–27.5 C. A full
  duty write/readback passed. Lower fan duty has not been validated.
- RTC ds1672 enumerates, but its oscillator is disabled; no clock write made.
- Factory DT LM90@4c and mux@77 probes fail; populated hardware unconfirmed.
- PAMU emits a late-probe warning for the PCI host. This remains recorded;
  it is not proof that arbitrary ASIC DMA works.

- PCI IRQs are 43 and 17 on the final full-memory cold boot. RX DMA, frame
  physical, and driver RX errors remain zero after the transfer. Stack RX-drop
  counters do rise; this is not a clean-room loss/throughput qualification.
- Initial 18-second DHCP attempt timed out; retry later succeeded. RAM init now
  allows 90 seconds for negotiation/upstream port readiness. Final cold boot
  gets 10.60.2.253 automatically and passes a further 3/3 workstation ping.

## artifacts and access

See [image report](nixos-image-report.json), [NixOS build notes](nixos.md) and
[thermal evidence](thermal.md). `nixos-build/` holds ignored images and build
logs. `load_ram_probe.py` transfers only public build files through TFTP;
`powercycle_to_uboot.py` uses the existing lab outlet and stops autoboot. Neither
saves U-Boot variables. Original images and environment data stay protected.

Captures: [first clean boot](linux-7.3.4-first-boot-20261114.txt),
[DT bring-up](linux-7.3.4-dt-boot-20261114.txt),
[tool diagnostics](linux-7.3.4-tools-boot-20261114.txt),
[LIODN trial](linux-7.3.4-liodn-boot-20261114.txt),
[low-memory/RX-hash trial](linux-7.3.4-lowmem-rxhash-20261114.txt),
[full-memory network verification](linux-7.3.4-network-boot-20261114.txt),
[final unattended boot](linux-7.3.4-final-boot-20261114.txt).

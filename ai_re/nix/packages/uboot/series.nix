# ONE ordered series: later patches rely on the context of earlier ones, so
# the numbering is the apply order. scope says who can reuse a patch:
#   t208x  - any T208x / e6500 board built from the T208xRDB port
#   as7760 - Accton AS7760-32X only (DDR tuning, DT, SPD addresses, identity)
#   as7772 - Accton AS7772-32X: applied on top of the as7760 ones (name, DT, DDR HSE bit + wrlvl starts)
#   fsl-sata - any board with the Freescale SATA controller: standard boot from its disks
#   alpine - any Annapurna Alpine board (V2, V3): DW SSI chip select
#   spi-nor-ro - any board: read-only SPI NOR with an optional write window
#   alpine-v2 - any Annapurna Alpine V2 board started by the vendor U-Boot or by al-boot stage3
#   dmc12  - Edgecore DMC-12-48R-AS0 only (CPLD reset, U-Boot DT bits, defconfig)
#   alpine-v3 - any Annapurna Alpine V3 board after al-boot stage3 (target; generic
#            fixes the pinned V2 builds must not see)
#   cs8210 - Celestica CS8210-32X only (U-Boot DT bits, defconfig)
#   armada8k - any Marvell Armada 7040/8040 (AP806 + CP110) board; boards
#            bring their control DTS to ./armada8k (no per-board patch)
# A sibling board takes the t208x ones and replaces the as7760 ones.
# Each build applies only its scopes (patchesFor in ./default.nix). The series
# as a whole is NOT one linear branch: scopes that are never built together
# may edit the same lines (alpine-v2 and alpine-v3 both add their PCIe driver
# at one spot of drivers/pci/Makefile, so 0030 does not apply after 0017).
# Pure data, no pkgs: ./variants.nix reads it too. patchesFor checks that it
# lists exactly the files of ./patches, numbered 0001.. in this order.
[
  { scope = "t208x"; file = "0001-mpc85xx-optionally-skip-CCSR-relocation-for-chainloa.patch"; }
  { scope = "t208x"; file = "0002-board-t208xrdb-honour-CONFIG_VID-and-CONFIG_PCI.patch"; }
  { scope = "t208x"; file = "0003-configs-T208xRDB-provide-the-NS16550-clock-for-DM-se.patch"; }
  { scope = "t208x"; file = "0004-powerpc-dts-t2080rdb-make-i2c0-available-before-relo.patch"; }
  { scope = "as7760"; file = "0005-AS7760-test-hack-SPD-EEPROMs-at-0x52-0x53.patch"; }
  { scope = "as7760"; file = "0006-powerpc-dts-add-Accton-AS7760-32X-control-tree.patch"; }
  { scope = "t208x"; file = "0007-mpc85xx-chainload-restart-secondaries-from-our-boot-.patch"; }
  { scope = "t208x"; file = "0008-board-t208xrdb-report-all-DDR-to-the-OS.patch"; }
  { scope = "t208x"; file = "0009-ddr-fsl-honour-the-board-CPO-in-the-A009942-workaround.patch"; }
  { scope = "as7760"; file = "0010-board-t208xrdb-AS7760-32X-DDR-setup-as-the-factory-does.patch"; }
  { scope = "t208x"; file = "0011-board-t208xrdb-add-ddrcmp-bring-up-command.patch"; }
  { scope = "t208x"; file = "0012-board-t208xrdb-add-spltest-bring-up-command.patch"; }
  { scope = "as7760"; file = "0013-board-t208xrdb-Accton-AS7760-32X-board-identity.patch"; }
  { scope = "alpine-v2"; file = "0014-arm-add-a-chainloaded-Annapurna-Labs-Alpine-V2-targe.patch"; }
  { scope = "alpine-v2"; file = "0015-board-alpine-v2-hand-the-al_eth-board-params-and-MAC.patch"; }
  { scope = "alpine-v2"; file = "0016-net-add-the-Annapurna-Labs-Alpine-V2-integrated-Ethe.patch"; }
  { scope = "alpine-v2"; file = "0017-pci-add-the-Annapurna-Labs-Alpine-V2-external-PCIe-r.patch"; }
  { scope = "alpine"; file = "0018-spi-designware-Annapurna-Alpine-chip-select-override.patch"; }
  { scope = "spi-nor-ro"; file = "0019-mtd-spi-read-only-SPI-NOR-with-an-optional-write-win.patch"; }
  { scope = "alpine-v2"; file = "0020-board-alpine-v2-SoC-init-from-the-DT-in-place-of-the.patch"; }
  { scope = "alpine-v2"; file = "0021-board-alpine-v2-say-where-the-environment-came-from.patch"; }
  { scope = "dmc12"; file = "0022-sysreset-add-the-Edgecore-DMC-12-board-CPLD-reset.patch"; }
  { scope = "dmc12"; file = "0023-board-Edgecore-DMC-12-48R-AS0-on-the-Alpine-V2-targe.patch"; }
  { scope = "alpine-v3"; file = "0024-arm-add-an-Annapurna-Labs-Alpine-V3-target-after-al-.patch"; }
  { scope = "alpine-v3"; file = "0025-spi-mem-read-only-build-optionally-pass-the-4-byte-a.patch"; }
  { scope = "cs8210"; file = "0026-board-Celestica-CS8210-32X-on-the-Alpine-V3-target.patch"; }
  { scope = "alpine-v3"; file = "0027-mtd-spi-write-window-up-to-six-ranges-4-byte-chips-s.patch"; }
  { scope = "cs8210"; file = "0028-board-CS8210-32X-installer-build-boot-SPI-write-wind.patch"; }
  { scope = "alpine-v3"; file = "0029-board-alpine-v3-SoC-init-from-the-DT-in-place-of-the.patch"; }
  { scope = "alpine-v3"; file = "0030-pci-add-the-Annapurna-Labs-Alpine-V3-external-PCIe-r.patch"; }
  { scope = "cs8210"; file = "0031-board-CS8210-32X-what-the-vendor-U-Boot-s-board-cfg-.patch"; }
  { scope = "armada8k"; file = "0032-arm-mvebu-armada8k-map-all-DRAM-in-the-MMU.patch"; }
  { scope = "armada8k"; file = "0033-arm-mvebu-armada8k-build-without-networking-or-an-MM.patch"; }
  { scope = "as7772"; file = "0034-board-t208xrdb-Accton-AS7772-32X-on-the-AS7760-port.patch"; }
  { scope = "fsl-sata"; file = "0035-ata-fsl_sata-create-a-bootdev-for-each-disk.patch"; }
]

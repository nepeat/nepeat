{ pkgs }:
# Upstream U-Boot (nixpkgs buildUBoot, 2026.10) for the Accton AS7760-32X
# (T2080). Board deltas are patches + Kconfig fragments; see devices/edge-core-7760-32x/u-boot-port.md.
# The same series also carries the Alpine V2 (arm64) patches, built in
# ./alpine-v2/; each build takes only its own scopes.
#
# Toolchain: 32-bit PowerPC Linux-ABI cross gcc without a target libc:
#  - ppc-embedded (eabi) libgcc will not link with U-Boot's -mrelocatable
#  - the pinned ppc32 cross glibc fails in "make install" (manual/ runs the
#    libc.so.6 rule twice) -- and U-Boot needs no libc.
let
  cross = pkgs.pkgsCross.ppc32;
  inherit ((cross.callPackage (cross.path + "/pkgs/misc/uboot") { stdenv = cross.stdenvNoLibc; })) buildUBoot;
  # ONE ordered series: later patches rely on the context of earlier ones, so
  # the numbering is the apply order. scope says who can reuse a patch:
  #   t208x  - any T208x / e6500 board built from the T208xRDB port
  #   as7760 - Accton AS7760-32X only (DDR tuning, DT, SPD addresses, identity)
  #   alpine-v2 - any Annapurna Alpine V2 board chainloaded by the vendor U-Boot
  #   dmc12  - Edgecore DMC-12-48R-AS0 only (CPLD reset, U-Boot DT bits, defconfig)
  # A sibling board takes the t208x ones and replaces the as7760 ones.
  # Platforms touch disjoint files: each build applies only its scopes, and
  # the whole series still applies in order to a pristine tree.
  series = [
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
    { scope = "dmc12"; file = "0015-sysreset-add-the-Edgecore-DMC-12-board-CPLD-reset.patch"; }
    { scope = "dmc12"; file = "0016-board-Edgecore-DMC-12-48R-AS0-on-the-Alpine-V2-chain.patch"; }
    { scope = "alpine-v2"; file = "0017-board-alpine-v2-boot-the-initrd-and-FDT-in-place.patch"; }
  ];
  patchesFor = scopes: map (s: builtins.path { path = ./patches + "/${s.file}"; name = s.file; })
    (builtins.filter (s: builtins.elem s.scope scopes) series);
  # builtins.path: stable store names, so unrelated edits in this tree do not
  # rebuild U-Boot.
  patches = patchesFor [ "t208x" "as7760" ];
  mk = { defconfig, extraConfig ? "", patches ? [ ], files ? [ "u-boot.bin" "u-boot" "u-boot.map" ".config" ] }:
    buildUBoot {
      inherit defconfig extraConfig;
      extraPatches = patches;
      extraMeta.platforms = [ "powerpc-linux" ];
      filesToInstall = files;
    };

  # Board settings shared by every AS7760 image.
  as7760Config = ''
    # SYSCLK is 100 MHz (CCB 600 MHz = 6x); T2080 default is 66.66 MHz.
    CONFIG_SYS_CLK_FREQ=100000000
    # No IR36021 VID regulator; misc_init_r only pokes the RDB CPLD.
    # CONFIG_VID is not set
    # CONFIG_MISC_INIT_R is not set
    CONFIG_DEFAULT_DEVICE_TREE="as7760-32x"
    CONFIG_OF_LIST="as7760-32x"
    CONFIG_PHY_BROADCOM=y
    # Two dual-rank DDR3 UDIMMs (SPD 0x52/0x53), factory-matched DDR setup.
    CONFIG_DIMM_SLOTS_PER_CTLR=2
    CONFIG_AS7760_DDR=y
    # pre-reloc DM I2C probe for SPD needs more than the stock 1 KiB.
    CONFIG_SYS_MALLOC_F_LEN=0x1000
    # No RDB CPLD / ID EEPROM on this board (patch 0013).
    CONFIG_AS7760_BOARD=y
    # CONFIG_ID_EEPROM is not set
    # Standard boot: extlinux + boot.scr on any bootdev, PXE, and the
    # bootdev/bootmeth/bootstd commands (not defaulted: the defconfig's
    # "not set" lines win over BOOTSTD_FULL's defaults).
    CONFIG_BOOTSTD_FULL=y
    CONFIG_BOOTSTD_DEFAULTS=y
    CONFIG_BOOTMETH_SCRIPT=y
    CONFIG_CMD_BOOTDEV=y
    CONFIG_CMD_BOOTMETH=y
    CONFIG_CMD_BOOTSTD=y
    CONFIG_SUPPORT_RAW_INITRD=y
    CONFIG_CMD_DDRCMP=y
    # No USB/SATA devices populated (SATA would also add a slow, useless
    # "ahci" hunt to every bootflow scan).
    # CONFIG_USB is not set
    # CONFIG_FSL_SATA_V2 is not set
    # CONFIG_SATA is not set
    # CONFIG_CMD_SATA is not set
    # CONFIG_FSL_SATA is not set
    # CONFIG_AHCI is not set
  '';
  # Used when the saved environment is missing/corrupt; normally the factory
  # environment (same flash location) supplies bootcmd -> nos_bootcmd.
  # Load addresses come from the board code (patch 0013).
  bootCommand = "bootflow scan -lb";
in {
  # For other boards: the builder and the raw series.
  lib = { inherit mk series patchesFor; };

  # Stock reference-board NOR image. Build-only; never flash (VID on).
  t2080rdb-nor = mk { defconfig = "T2080RDB_defconfig"; };

  # THE replacement: stock NOR reset path at 0xEFF40000 (mtd7), environment
  # in NOR at 0xEFF20000 (mtd6, factory format/location), FMan ucode from
  # 0xEFF00000 (mtd5). Does the cold DDR init itself.
  as7760-nor = mk {
    defconfig = "T2080RDB_defconfig";
    inherit patches;
    extraConfig = as7760Config + ''
      CONFIG_LOCALVERSION="-as7760"
      CONFIG_BOOTDELAY=3
      CONFIG_USE_BOOTCOMMAND=y
      CONFIG_BOOTCOMMAND="${bootCommand}"
    '';
  };

  # Stage 2: SDCARD's RAM-linked second stage (TEXT_BASE 0x00200000), started
  # with "go" from the factory U-Boot (nos_bootcmd). Also carries the SPL for
  # the spltest cold-DDR experiment.
  as7760-chainload = mk {
    defconfig = "T2080RDB_SDCARD_defconfig";
    inherit patches;
    files = [ "u-boot.bin" "u-boot" "u-boot.map" ".config" "spl/u-boot-spl.bin" "spl/u-boot-spl" ];
    extraConfig = as7760Config + ''
      CONFIG_CHAINLOAD_CCSR_RELOCATED=y
      CONFIG_LOCALVERSION="-as7760-chainload"
      CONFIG_CMD_SPLTEST=y
      # SPL for spltest: keep CCSR where the previous loader put it.
      # CONFIG_SPL_SYS_CCSR_DO_NOT_RELOCATE is not set
      # FMan microcode from the board's NOR "Fman-FW" partition (read-only).
      # CONFIG_SYS_QE_FMAN_FW_IN_MMC is not set
      CONFIG_SYS_QE_FMAN_FW_IN_NOR=y
      CONFIG_SYS_FMAN_FW_ADDR=0xEFF00000
      # env stays in MMC (SPL needs ENV_OFFSET): 1 MiB lies in the zeroed gap
      # before the first SD partition -> CRC fails -> defaults. Never saveenv.
      CONFIG_BOOTDELAY=2
      CONFIG_USE_BOOTCOMMAND=y
      CONFIG_BOOTCOMMAND="env import -c 0xeff20000 0x2000 ethaddr eth1addr eth2addr serial#; ${bootCommand}"
    '';
  };

  # Same toolchain/U-Boot tree on QEMU's ppce500 machine, run with -cpu e6500.
  qemu-ppce500 = mk { defconfig = "qemu-ppce500_defconfig"; files = [ "u-boot" "u-boot.bin" ".config" ]; };
}

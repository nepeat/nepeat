{ pkgs }:
# Upstream U-Boot (nixpkgs buildUBoot, 2026.10): the shared patch series
# (./series.nix, ./patches) and the NXP T208x (ppc) builds: Accton AS7760-32X
# and AS7772-32X. Board deltas are patches + Kconfig fragments; see
# devices/edge-core-7760-32x/u-boot-port.md. The arm64 platforms build from
# the same series in ./alpine-v2, ./alpine-v3 and ./armada8k; each build
# takes only its own scopes. README.md has the patch table and the rules.
#
# Toolchain: 32-bit PowerPC Linux-ABI cross gcc without a target libc:
#  - ppc-embedded (eabi) libgcc will not link with U-Boot's -mrelocatable
#  - the pinned ppc32 cross glibc fails in "make install" (manual/ runs the
#    libc.so.6 rule twice) -- and U-Boot needs no libc.
let
  inherit (pkgs) lib;
  cross = pkgs.pkgsCross.ppc32;
  inherit ((cross.callPackage (cross.path + "/pkgs/misc/uboot") { stdenv = cross.stdenvNoLibc; })) buildUBoot;
  series = import ./series.nix;
  scopes = lib.unique (map (s: s.scope) series);
  # The series is rebased for exactly this upstream release, which nixpkgs
  # picks (buildUBoot's default). After a nixpkgs bump that moves it, patches
  # can still apply with fuzz and give a different image without any error:
  # rebase the series (README.md), re-test, then change this.
  version = "2026.10";
  seriesOk =
    let files = map (s: s.file) series; in
    lib.assertMsg (pkgs.ubootTools.version == version)
      "uboot: nixpkgs builds U-Boot ${pkgs.ubootTools.version}, the patch series is for ${version} (packages/uboot/default.nix)"
    && lib.assertMsg (files == builtins.attrNames (builtins.readDir ./patches))
      "uboot: series.nix and the files in patches/ differ, or the series is not in number order"
    && lib.assertMsg (lib.all lib.id (lib.imap1 (i: f: lib.hasPrefix "${lib.fixedWidthNumber 4 i}-" f) files))
      "uboot: patch numbers are not 0001..${lib.fixedWidthNumber 4 (builtins.length files)} without gaps";
  # A misspelt scope would otherwise select nothing, silently.
  patchesFor = wanted:
    assert seriesOk;
    assert lib.assertMsg (lib.all (s: builtins.elem s scopes) wanted)
      "uboot.patchesFor: unknown scope in [ ${toString wanted} ]; known: ${toString scopes}";
    # builtins.path: stable store names, so unrelated edits in this tree do
    # not rebuild U-Boot.
    map (s: builtins.path { path = ./patches + "/${s.file}"; name = s.file; })
      (builtins.filter (s: builtins.elem s.scope wanted) series);
  # Same bytes from any build host, for every board: CC_VERSION_STRING would carry the cross
  # prefix ("aarch64-unknown-linux-gnu-gcc (GCC) ..." vs "gcc (GCC) ..." natively) and shift the
  # whole image. dmc12-firmware pins its payload by sha256, so this is load-bearing there.
  reproducibleVersion = ''
    sed -i 's#$(CC) --version | head -n 1)#$(CC) --version | head -n 1 | sed -e "s/^[^ ]*-gcc /gcc /")#' Makefile
    grep -q 's/^\[^ \]\*-gcc /gcc /' Makefile
  '';

  mk = { defconfig, extraConfig ? "", patches ? [ ], files ? [ "u-boot.bin" "u-boot" "u-boot.map" ".config" ] }:
    (buildUBoot {
      inherit defconfig extraConfig;
      extraPatches = patches;
      extraMeta.platforms = [ "powerpc-linux" ];
      filesToInstall = files;
    }).overrideAttrs (o: { postPatch = (o.postPatch or "") + reproducibleVersion; });

  # arm64 builder of the Alpine boards (./alpine-v2, ./alpine-v3), all RAM
  # images at loadAddr: the board's control DTS is copied into arch/arm/dts,
  # and u-boot.bin is also wrapped as a kernel-type legacy uImage: the vendor
  # "bootm" then enters us like Linux (MMU and caches off, EL2, x0 = its
  # DTB); -T standalone would not.
  mkAlpine = { patches, loadAddr, postInstall ? "" }:
    { defconfig, dtsName, dts, extraConfig ? "", uimageName }:
    (pkgs.pkgsCross.aarch64-multiplatform.buildUBoot {
      inherit defconfig extraConfig;
      extraPatches = patches;
      extraMeta.platforms = [ "aarch64-linux" ];
      filesToInstall = [ "u-boot.bin" "u-boot" "u-boot.map" "u-boot.dtb" ".config" ];
    }).overrideAttrs (o: {
      postPatch = o.postPatch + ''
        cp ${dts} arch/arm/dts/${dtsName}.dts
      '' + reproducibleVersion;
      postInstall = ''
        ./tools/mkimage -A arm64 -O linux -T kernel -C none \
          -a ${loadAddr} -e ${loadAddr} -n "${uimageName}" \
          -d u-boot.bin $out/u-boot.uimage
        ./tools/mkimage -l $out/u-boot.uimage > $out/u-boot.uimage.txt
      '' + postInstall;
    });

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
  # AS7772-32X = the AS7760 board + MACsec PHYs, TPM, M.2 SATA SSD (patch 0034).
  # Later lines win over as7760Config's: own control tree, and SATA back on
  # (the SSD on SATA2 is the boot disk).
  as7772Config = as7760Config + ''
    CONFIG_AS7772_BOARD=y
    CONFIG_DEFAULT_DEVICE_TREE="as7772-32x"
    CONFIG_OF_LIST="as7772-32x"
    CONFIG_SATA=y
    CONFIG_FSL_SATA=y
    CONFIG_FSL_SATA_V2=y
    CONFIG_SYS_SATA_MAX_DEVICE=2
    CONFIG_CMD_SATA=y
    CONFIG_EFI_PARTITION=y
  '';
  # Used when the saved environment is missing/corrupt; normally the factory
  # environment (same flash location) supplies bootcmd -> nos_bootcmd.
  # Load addresses come from the board code (patch 0013).
  bootCommand = "bootflow scan -lb";

  # A board of this family is { name, patches, config }; each gets the same
  # two images.
  as7760 = { name = "as7760"; patches = patchesFor [ "t208x" "as7760" ]; config = as7760Config; };
  as7772 = { name = "as7772"; patches = patchesFor [ "t208x" "as7760" "as7772" "fsl-sata" ]; config = as7772Config; };

  # THE replacement: stock NOR reset path at 0xEFF40000 (mtd7), environment
  # in NOR at 0xEFF20000 (mtd6, factory format/location), FMan ucode from
  # 0xEFF00000 (mtd5). Does the cold DDR init itself.
  norImage = { name, patches, config }: mk {
    defconfig = "T2080RDB_defconfig";
    inherit patches;
    extraConfig = config + ''
      CONFIG_LOCALVERSION="-${name}"
      CONFIG_BOOTDELAY=3
      CONFIG_USE_BOOTCOMMAND=y
      CONFIG_BOOTCOMMAND="${bootCommand}"
    '';
  };

  # Stage 2: SDCARD's RAM-linked second stage (TEXT_BASE 0x00200000), started
  # with "go" from the factory U-Boot (nos_bootcmd). The env comment below is
  # the AS7760's case; on a board without a card the read fails instead, with
  # the same result (default env).
  chainloadImage = { name, patches, config, extraConfig ? "", extraFiles ? [ ] }: mk {
    defconfig = "T2080RDB_SDCARD_defconfig";
    inherit patches;
    files = [ "u-boot.bin" "u-boot" "u-boot.map" ".config" ] ++ extraFiles;
    extraConfig = config + ''
      CONFIG_CHAINLOAD_CCSR_RELOCATED=y
      CONFIG_LOCALVERSION="-${name}-chainload"
    '' + extraConfig + ''
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
in {
  # For other boards: the builders, the series and the release it is for.
  lib = { inherit mk mkAlpine series scopes patchesFor reproducibleVersion version; };

  # Stock reference-board NOR image. Build-only; never flash (VID on).
  t2080rdb-nor = mk { defconfig = "T2080RDB_defconfig"; };

  as7760-nor = norImage as7760;
  # Also carries the SPL for the spltest cold-DDR experiment.
  as7760-chainload = chainloadImage (as7760 // {
    extraFiles = [ "spl/u-boot-spl.bin" "spl/u-boot-spl" ];
    extraConfig = ''
      CONFIG_CMD_SPLTEST=y
      # SPL for spltest: keep CCSR where the previous loader put it.
      # CONFIG_SPL_SYS_CCSR_DO_NOT_RELOCATE is not set
    '';
  });

  # AS7772-32X: the same two images (devices/edge-core-as7772-32x/u-boot-port.md).
  as7772-nor = norImage as7772;
  as7772-chainload = chainloadImage as7772;

  # Same toolchain/U-Boot tree on QEMU's ppce500 machine, run with -cpu e6500.
  qemu-ppce500 = mk { defconfig = "qemu-ppce500_defconfig"; files = [ "u-boot" "u-boot.bin" ".config" ]; };
}

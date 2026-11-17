{ pkgs, uboots }:
# Upstream U-Boot (nixpkgs buildUBoot, 2026.10) for Annapurna Labs Alpine V3
# boards, run after al-boot stage3: for now started from RAM by the vendor
# U-Boot 2018.05 (loadx + booti/bootm/go). Patches: the "alpine" +
# "alpine-v3" + "cs8210" scopes of the shared series in ../default.nix.
# Plan, load addresses and test status: devices/cs8210-32x/u-boot-port.md.
#
# The board DTS is the Linux one: ./alpine-v3-cs8210.dts is a byte-for-byte
# copy of nepeat/nix nixos/module/cs8210/kernel/dts/amazon/alpine-v3-cs8210.dts
# (that flake owns it; a path: flake here cannot read it). u-boot-port.md has
# the cmp that keeps them equal. U-Boot-only nodes live in the series'
# alpine-v3-cs8210-u-boot.dtsi.
let
  cross = pkgs.pkgsCross.aarch64-multiplatform;
  patches = uboots.lib.patchesFor [ "alpine" "alpine-v3" "cs8210" ];
  # Link/load address: the vendor "loadaddr" (ONIE's bootm buffer). The
  # vendor U-Boot sits at 0x01100000 and, relocated, 0x03ac0000-0x04000000;
  # its boot.scr loads to 0x07000000. The image (code + BSS + early stack) is
  # well under 2 MiB; tests.nix checks that.
  loadAddr = "0x08000000";

  cs8210Dts = builtins.path { path = ./alpine-v3-cs8210.dts; name = "alpine-v3-cs8210.dts"; };

  mk = { defconfig, dtsName, dts, extraConfig ? "", uimageName }:
    (cross.buildUBoot {
      inherit defconfig extraConfig;
      extraPatches = patches;
      extraMeta.platforms = [ "aarch64-linux" ];
      filesToInstall = [ "u-boot.bin" "u-boot" "u-boot.map" "u-boot.dtb" ".config" ];
    }).overrideAttrs (o: {
      postPatch = o.postPatch + ''
        cp ${dts} arch/arm/dts/${dtsName}.dts
      '' + uboots.lib.reproducibleVersion;
      # Kernel-type legacy uImage, for the vendor "bootm": it then enters us
      # like Linux (MMU and caches off, EL2, x0 = its DTB). u-boot.bin
      # itself carries a Linux Image header for the vendor "booti".
      postInstall = ''
        ./tools/mkimage -A arm64 -O linux -T kernel -C none \
          -a ${loadAddr} -e ${loadAddr} -n "${uimageName}" \
          -d u-boot.bin $out/u-boot.uimage
        ./tools/mkimage -l $out/u-boot.uimage > $out/u-boot.uimage.txt
        (cd $out && sha256sum u-boot.bin u-boot.uimage > SHA256SUMS)
      '';
    });

  # Kconfig of cs8210-selfinit on top of alpine_v3_cs8210_defconfig: what the
  # vendor U-Boot does before its prompt and al-boot stage3 does not (patches
  # 0029-0031: MUIO, GPIO, SB PLL, base MAC, external PCIe RC init), and a
  # redundant env in blank boot SPI behind the one write window (0019/0027).
  selfinitConfig = ''
    CONFIG_LOCALVERSION="-cs8210-selfinit"
    CONFIG_ALPINE_V3_MUIO_INIT=y
    CONFIG_ALPINE_V3_GPIO_INIT=y
    CONFIG_ALPINE_V3_PLL_INIT=y
    CONFIG_ALPINE_V3_ETH_MAC=y
    CONFIG_CMD_ALPINE_V3=y
    CONFIG_PCIE_ALPINE_V3=y
    CONFIG_CMD_ALPINE_V3_PCIE=y
    # Env: 2 x 16 KiB at 0x38000/0x3c000 (blank in the 0x01c000-0x040000 gap
    # after stage2; clear of the A' TOC2 0x020000 and BOOT_MODE 0x030000
    # sectors), 4 KiB sectors, 4-byte opcodes. The factory env (0x380000)
    # stays the vendor U-Boot's. Bad CRC / blank: the built-in default env.
    CONFIG_SPI_FLASH_WRITE_WINDOW=y
    CONFIG_SPI_FLASH_WRITE_WINDOW_BUS=0
    CONFIG_SPI_FLASH_WRITE_WINDOW_CS=0
    CONFIG_SPI_FLASH_WRITE_WINDOW_JEDEC=0xc22539
    CONFIG_SPI_FLASH_WRITE_WINDOW_CHIP_SIZE=0x2000000
    CONFIG_SPI_FLASH_WRITE_WINDOW_ADDR_WIDTH=4
    CONFIG_SPI_FLASH_WRITE_WINDOW_START=0x38000
    CONFIG_SPI_FLASH_WRITE_WINDOW_SIZE=0x8000
    # CONFIG_ENV_IS_NOWHERE is not set
    CONFIG_ENV_IS_IN_SPI_FLASH=y
    CONFIG_ENV_REDUNDANT=y
    CONFIG_ENV_OFFSET=0x38000
    CONFIG_ENV_OFFSET_REDUND=0x3c000
    CONFIG_ENV_SIZE=0x4000
    CONFIG_ENV_SECT_SIZE=0x1000
    CONFIG_ENV_SPI_BUS=0
    CONFIG_ENV_SPI_CS=0
    CONFIG_ENV_SPI_MAX_HZ=25000000
    CONFIG_ENV_SPI_MODE=0x0
    CONFIG_CMD_SAVEENV=y
    # Size: the stage3 app must fit A' slot B (0x2f9000, 412 KiB) as well as A.
    # CONFIG_CMD_FDT is not set
    # CONFIG_CMD_LZMADEC is not set
    # CONFIG_LZMA is not set
    # CONFIG_CMD_UNZIP is not set
    # CONFIG_CMD_LOADS is not set
    # CONFIG_CMD_ELF is not set
    # CONFIG_CMD_XIMG is not set
    # CONFIG_CMD_MEMINFO is not set
  '';
  selfinit = mk {
    defconfig = "alpine_v3_cs8210_defconfig";
    dtsName = "alpine-v3-cs8210";
    dts = cs8210Dts;
    uimageName = "U-Boot 2026.10 cs8210 selfinit";
    extraConfig = selfinitConfig;
  };
  alboot = builtins.path { path = ../../alboot/alboot.py; name = "alboot.py"; };
in {
  inherit loadAddr patches;

  # The chainload build plus the vendor board init (selfinitConfig). One
  # binary for both entries: RAM-tested from the vendor prompt (a link the
  # vendor trained is kept, the rest is idempotent; "alpcie reinit" redoes
  # a port) and as al-boot stage3's application (cs8210-stage3app).
  cs8210-selfinit = selfinit;

  # cs8210-selfinit's u-boot.bin, byte for byte, as an al-boot application
  # object (stage3 loads to and enters at the header's load/exec): id 0x110,
  # load = exec = 0x08000000 (TEXT_BASE; PIE, the RAM-tested address). The
  # installer (cs8210-installer, spi_upgrade.py) writes it to the A' slots.
  cs8210-stage3app = pkgs.runCommand "uboot-cs8210-stage3app" {
    nativeBuildInputs = [ pkgs.python3 ];
  } ''
    set -euo pipefail
    mkdir $out
    cp ${selfinit}/u-boot.bin ${selfinit}/u-boot.dtb ${selfinit}/.config $out/
    python3 ${alboot} wrap --id 0x110 --load ${loadAddr} --version 2026.10.0 \
      ${selfinit}/u-boot.bin $out/u-boot-app-08000000.fimg | tee $out/wrap.txt
    python3 ${alboot} check $out/u-boot-app-08000000.fimg | tee $out/check.txt
    s=$(stat -c %s $out/u-boot-app-08000000.fimg)
    # A' slot B is the smaller one: 0x2f9000-0x360000
    [ $s -le $((0x67000)) ] || { echo "object $s B > slot B 0x67000"; exit 1; }
    echo "fits slot B (0x67000) with $((0x67000 - s)) B spare, slot A (0xa0000) with $((0xa0000 - s))" | tee -a $out/check.txt
    (cd $out && sha256sum u-boot.bin *.fimg > SHA256SUMS)
  '';

  # RAM build for the CS8210-32X, started by the vendor U-Boot. Never flashed.
  cs8210 = mk {
    defconfig = "alpine_v3_cs8210_defconfig";
    dtsName = "alpine-v3-cs8210";
    dts = cs8210Dts;
    uimageName = "U-Boot 2026.10 cs8210";
    extraConfig = ''
      CONFIG_LOCALVERSION="-cs8210"
    '';
  };

  # Installer: RAM build started from the vendor prompt like the one above,
  # with the boot SPI writable only in the scheme A' windows (patches 0027 +
  # 0028: TOC2 0x20000, BOOT_MODE 0x30000, app slots 0x0f0000 / 0x2f9000) and
  # "sf erase/write" for them; prompt "cs8210-installer> ", no autoboot.
  # Driven by devices/cs8210-32x/scripts/spi_upgrade.py (spi-upgrade-plan.md).
  cs8210-installer = mk {
    defconfig = "alpine_v3_cs8210_installer_defconfig";
    dtsName = "alpine-v3-cs8210";
    dts = cs8210Dts;
    uimageName = "U-Boot 2026.10 cs8210 installer";
    extraConfig = ''
      CONFIG_LOCALVERSION="-cs8210-installer"
    '';
  };

  # The installer's SPI code, window and prompt on U-Boot sandbox (native),
  # the sandbox flash emulating the MX25U25635F with 4-byte opcodes and
  # NOR program semantics (test-only ./sandbox-window/sandbox-sf-4b.patch,
  # never in a board build), backed by ./spi.bin of its working directory.
  # spi_upgrade.py rehearse runs the real install sequence against it.
  sandbox-installer = (pkgs.buildUBoot {
    defconfig = "sandbox_defconfig";
    extraConfig = builtins.readFile ./sandbox-window/sandbox.config;
    extraPatches = patches;
    extraMeta.platforms = [ "x86_64-linux" ];
    filesToInstall = [ "u-boot" "u-boot.dtb" ".config" ];
  }).overrideAttrs (o: {
    pname = "uboot-sandbox-cs8210-installer";
    postPatch = o.postPatch + uboots.lib.reproducibleVersion + ''
      patch -p1 < ${./sandbox-window/sandbox-sf-4b.patch}
      cp ${../alpine-v2/sandbox-window/sfwtest.c} cmd/sfwtest.c
      echo 'obj-$(CONFIG_SANDBOX) += sfwtest.o' >> cmd/Makefile
      substituteInPlace arch/sandbox/dts/sandbox.dtsi \
        --replace-fail '"spansion,m25p16", "jedec,spi-nor"' '"macronix,mx25u25635f", "jedec,spi-nor"'
    '';
  });

  # Same board code and config on QEMU virt (test fixture DT; ./qemu-virt.dts).
  qemu-virt = mk {
    defconfig = "alpine_v3_cs8210_defconfig";
    dtsName = "alpine-v3-qemu-virt";
    dts = builtins.path { path = ./qemu-virt.dts; name = "alpine-v3-qemu-virt.dts"; };
    uimageName = "U-Boot 2026.10 alpine-v3 qemu";
    extraConfig = ''
      CONFIG_LOCALVERSION="-alpine-v3-qemu"
      CONFIG_DEFAULT_DEVICE_TREE="alpine-v3-qemu-virt"
      CONFIG_DEFAULT_FDT_FILE="test.dtb"
      # CONFIG_DEBUG_UART is not set
      CONFIG_PL01X_SERIAL=y
    '';
  };
}

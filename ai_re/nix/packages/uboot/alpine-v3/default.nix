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
in {
  inherit loadAddr patches;

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

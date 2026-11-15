{ pkgs, uboots }:
# Upstream U-Boot (nixpkgs buildUBoot, 2026.10) for Annapurna Labs Alpine V2
# boards, started from RAM by the vendor U-Boot 2015.07 ("bootm" of the
# uImage, or "go"). Patches: the "alpine-v2" + "dmc12" scopes of the shared
# series in ../default.nix. Plan, load addresses and test status:
# devices/dmc-12-48r-as0/u-boot-port.md.
#
# The board DTS is the Linux one (packages/linux-alpine/boards/<board>/),
# copied into arch/arm/dts at build time; U-Boot-only nodes live in the
# patch series' <board>-u-boot.dtsi.
let
  cross = pkgs.pkgsCross.aarch64-multiplatform;
  patches = uboots.lib.patchesFor [ "alpine-v2" "dmc12" ];
  # Link/load address: the vendor "loadaddr". Below it: the vendor U-Boot
  # (relocated to 0x03F0A000, stack ~0x03B04CD0), its DTB copy at
  # 0x04078000, firmware at 0x0 and 0x02000000. Above it: vendor initrd
  # staging at 0x0fd00000. The image (code + BSS + 512 KiB early stack) is
  # well under 2 MiB; tests.nix checks that.
  loadAddr = "0x08000000";

  mk = { defconfig, dtsName, dts, extraConfig ? "", uimageName }:
    (cross.buildUBoot {
      inherit defconfig extraConfig;
      extraPatches = patches;
      extraMeta.platforms = [ "aarch64-linux" ];
      filesToInstall = [ "u-boot.bin" "u-boot" "u-boot.map" "u-boot.dtb" ".config" ];
    }).overrideAttrs (o: {
      postPatch = o.postPatch + ''
        cp ${dts} arch/arm/dts/${dtsName}.dts
      '';
      # Kernel-type legacy uImage: the vendor bootm then enters us like Linux
      # (MMU and caches off, EL2, x0 = its DTB). -T standalone would not.
      postInstall = ''
        ./tools/mkimage -A arm64 -O linux -T kernel -C none \
          -a ${loadAddr} -e ${loadAddr} -n "${uimageName}" \
          -d u-boot.bin $out/u-boot.uimage
        ./tools/mkimage -l $out/u-boot.uimage > $out/u-boot.uimage.txt
      '';
    });
in {
  inherit loadAddr;

  # RAM-only second stage for the DMC-12. Never flashed.
  dmc12-chainload = mk {
    defconfig = "alpine_v2_dmc12_defconfig";
    dtsName = "alpine-v2-dmc12";
    dts = builtins.path {
      path = ../../linux-alpine/boards/dmc12/alpine-v2-dmc12.dts;
      name = "alpine-v2-dmc12.dts";
    };
    uimageName = "U-Boot 2026.10 dmc12 chainload";
    extraConfig = ''
      CONFIG_LOCALVERSION="-dmc12-chainload"
    '';
  };

  # Same board code on QEMU virt (test fixture DT; see ./qemu-virt.dts).
  qemu-virt = mk {
    defconfig = "alpine_v2_dmc12_defconfig";
    dtsName = "alpine-v2-qemu-virt";
    dts = builtins.path { path = ./qemu-virt.dts; name = "alpine-v2-qemu-virt.dts"; };
    uimageName = "U-Boot 2026.10 alpine-v2 qemu";
    extraConfig = ''
      CONFIG_LOCALVERSION="-alpine-v2-qemu"
      CONFIG_DEFAULT_DEVICE_TREE="alpine-v2-qemu-virt"
      CONFIG_DEFAULT_FDT_FILE="test.dtb"
      # CONFIG_DEBUG_UART is not set
      CONFIG_PL01X_SERIAL=y
    '';
  };
}

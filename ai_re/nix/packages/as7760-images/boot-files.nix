{ pkgs, targetPkgs, config, kernelImage }:
pkgs.runCommand "as7760-legacy-boot-files" {
  nativeBuildInputs = [ pkgs.ubootTools pkgs.dtc pkgs.gzip pkgs.python3 ];
} ''
  mkdir -p $out
  cp ${kernelImage}/uImage $out/uImage
  export SOURCE_DATE_EPOCH=1
  mkimage -A ppc -O linux -T ramdisk -C gzip -a 0 -e 0 \
    -n 'AS7760 NixOS initrd' -d ${config.system.build.initialRamdisk}/initrd $out/uInitrd
  dtc -I dts -O dtb -o $out/as7760.dtb ${builtins.path { path = ../../packages/linux-qoriq-t2080/boards/as7760/as7760-vendor.dts; name = "as7760-vendor.dts"; }} 2> $out/dtc-diagnostics.txt
  cat > $out/boot-commands.txt <<'EOF'
  # RAM-only environment changes. No saveenv. MMC contents must have already
  # been deployed separately; these commands do not install an image.
  mmc rescan
  fatload mmc 0:1 0x10000000 uImage
  fatload mmc 0:1 0x20000000 uInitrd
  fatload mmc 0:1 0x08000000 as7760.dtb
  setenv bootargs ${pkgs.lib.concatStringsSep " " config.boot.kernelParams} init=${config.system.build.toplevel}/init
  bootm 0x10000000 0x20000000 0x08000000
  EOF
  # The board DT is a captured vendor tree, not a validated upstream port.
  # This manifest makes that limitation visible alongside the image.
  python3 ${./validate-boot.py} $out ${kernelImage}/kernel.bin
''

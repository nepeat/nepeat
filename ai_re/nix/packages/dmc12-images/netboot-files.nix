{ pkgs, config }:
# Volatile full-NixOS bundle for the DMC-12's factory U-Boot: TFTP + bootm,
# nothing written to SPI or SSD. Also the reimaging environment (reimage.py).
# Load plan: kernel uImage 0x10000000, initrd 0x20000000, DTB 0x04078000. The
# kernel is wrapped uncompressed (-C none): bootm only memmoves it to its load
# address 0x04080000, so the factory U-Boot's gunzip size limit never applies
# (the full-config 7.3.4 Image is 68 MiB, vs 24 MiB for the vendor's). The DTB
# must stay under 32 KiB (it ends where the kernel starts) and the Image must
# end below the 0x10000000 staging. 2026-11-15: the first layout (gzip uImage at
# 0x08000000) failed this check at build time.
let
  kernel = config.boot.kernelPackages.kernel;
  dtb = "${config.hardware.deviceTree.package}/${config.hardware.deviceTree.name}";
in
pkgs.runCommand "dmc12-netboot-files" {
  nativeBuildInputs = [ pkgs.ubootTools pkgs.coreutils ];
} ''
  set -euo pipefail
  mkdir -p $out
  export SOURCE_DATE_EPOCH=1
  imageBytes=$(stat -c %s ${kernel}/Image)
  echo "$imageBytes" > $out/Image.bytes
  if [ $(( 0x04080000 + imageBytes )) -ge $(( 0x10000000 )) ]; then
    echo "Image ($imageBytes bytes) at 0x04080000 would overrun the uImage staging at 0x10000000"; exit 1
  fi
  mkimage -A arm64 -O linux -T kernel -C none -a 0x04080000 -e 0x04080000 \
    -n "DMC-12 NixOS ${kernel.version}" -d ${kernel}/Image $out/uImage > /dev/null
  initrdBytes=$(stat -c %s ${config.system.build.netbootRamdisk}/initrd)
  [ $(( 0x20000000 + initrdBytes )) -lt $(( 0xb0000000 )) ] || { echo "initrd reaches reserved memory at 0xb0000000"; exit 1; }
  # Mixed payload (compressed stage-1 cpio + raw squashfs cpio): the kernel
  # unpacks it, U-Boot must not, so the legacy header says "none".
  mkimage -A arm64 -O linux -T ramdisk -C none -a 0 -e 0 \
    -n "DMC-12 NixOS netboot" -d ${config.system.build.netbootRamdisk}/initrd $out/uInitrd > /dev/null
  cp ${dtb} $out/dmc12.dtb
  [ $(stat -c %s $out/dmc12.dtb) -lt 32768 ] || { echo "DTB over 32 KiB collides with the kernel at 0x04080000"; exit 1; }
  echo -n "init=${config.system.build.toplevel}/init ${pkgs.lib.concatStringsSep " " config.boot.kernelParams}" > $out/bootargs.txt
  # The long part (bootargs, ~250 chars) lives in a TFTP'd U-Boot script, not on
  # the console: `source` doesn't go through the console's line buffer.
  {
    echo 'tftpboot 0x10000000 ''${dmc12dir}/uImage || exit'
    echo 'tftpboot 0x20000000 ''${dmc12dir}/uInitrd || exit'
    echo 'tftpboot 0x04078000 ''${dmc12dir}/dmc12.dtb || exit'
    echo 'setenv initrd_high 0xffffffff'
    echo 'setenv fdt_high 0xffffffff'
    echo "setenv bootargs '$(cat $out/bootargs.txt)'"
    echo 'bootm 0x10000000 0x20000000 0x04078000'
  } > $out/boot.cmd
  mkimage -A arm64 -O linux -T script -C none -n "DMC-12 NixOS netboot" -d $out/boot.cmd $out/boot.scr > /dev/null
  # Typed on the console, one per line (console.py); <SERVER> and <DIR> are
  # filled in by reimage.py. The last line boots. No saveenv anywhere.
  {
    echo "setenv autoload no"
    echo "dhcp"
    echo "setenv serverip <SERVER>"
    echo "setenv dmc12dir <DIR>"
    echo "tftpboot 0x07000000 <DIR>/boot.scr"
    echo "source 0x07000000"
  } > $out/uboot-commands.txt
  (cd $out && sha256sum uImage uInitrd dmc12.dtb boot.scr > SHA256SUMS)
  echo ${config.system.build.toplevel} > $out/toplevel
''

{ pkgs, config }:
# Volatile full-NixOS bundle for the DMC-12's factory U-Boot: TFTP + bootm,
# nothing written to SPI or SSD. Also the reimaging environment (reimage.py).
# Load plan (factory env values): kernel uImage 0x08000000, initrd 0x0fd00000,
# DTB 0x04078000; the gzip kernel unpacks to 0x04080000, so the DTB must stay
# under 32 KiB, and the unpacked Image must fit below the 0x08000000 staging.
let
  kernel = config.boot.kernelPackages.kernel;
  dtb = "${config.hardware.deviceTree.package}/${config.hardware.deviceTree.name}";
in
pkgs.runCommand "dmc12-netboot-files" {
  nativeBuildInputs = [ pkgs.ubootTools pkgs.gzip pkgs.coreutils ];
} ''
  set -euo pipefail
  mkdir -p $out
  export SOURCE_DATE_EPOCH=1
  imageBytes=$(stat -c %s ${kernel}/Image)
  echo "$imageBytes" > $out/Image.bytes
  if [ $(( 0x04080000 + imageBytes )) -ge $(( 0x08000000 )) ]; then
    echo "unpacked Image ($imageBytes bytes) would overrun the uImage staging at 0x08000000"; exit 1
  fi
  gzip -9n -c ${kernel}/Image > Image.gz
  mkimage -A arm64 -O linux -T kernel -C gzip -a 0x04080000 -e 0x04080000 \
    -n "DMC-12 NixOS ${kernel.version}" -d Image.gz $out/uImage > /dev/null
  # Mixed payload (compressed stage-1 cpio + raw squashfs cpio): the kernel
  # unpacks it, U-Boot must not, so the legacy header says "none".
  mkimage -A arm64 -O linux -T ramdisk -C none -a 0 -e 0 \
    -n "DMC-12 NixOS netboot" -d ${config.system.build.netbootRamdisk}/initrd $out/uInitrd > /dev/null
  cp ${dtb} $out/dmc12.dtb
  [ $(stat -c %s $out/dmc12.dtb) -lt 32768 ] || { echo "DTB over 32 KiB collides with the kernel at 0x04080000"; exit 1; }
  echo -n "init=${config.system.build.toplevel}/init ${pkgs.lib.concatStringsSep " " config.boot.kernelParams}" > $out/bootargs.txt
  test "$(wc -c < $out/bootargs.txt)" -lt 240 || { echo "bootargs too long for the U-Boot command line"; exit 1; }
  # One command per line for console.py; <SERVER> and <DIR> are filled in by
  # reimage.py. No saveenv anywhere.
  {
    echo "setenv autoload no"
    echo "dhcp"
    echo "setenv serverip <SERVER>"
    echo "tftpboot 0x08000000 <DIR>/uImage"
    echo "tftpboot 0x0fd00000 <DIR>/uInitrd"
    echo "tftpboot 0x04078000 <DIR>/dmc12.dtb"
    echo "setenv initrd_high 0xffffffff"
    echo "setenv fdt_high 0xffffffff"
    echo "setenv bootargs $(cat $out/bootargs.txt)"
    echo "bootm 0x08000000 0x0fd00000 0x04078000"
  } > $out/uboot-commands.txt
  (cd $out && sha256sum uImage uInitrd dmc12.dtb > SHA256SUMS)
  echo ${config.system.build.toplevel} > $out/toplevel
''

{ pkgs, targetPkgs, kernelImage, fanDriver, kernel }:
# Small disposable root for the first hardware boot. No block mounts,
# installer, discovery daemon or persistent environment changes.
let
  diagnostics = [ targetPkgs.ethtool targetPkgs.pciutils targetPkgs.usbutils targetPkgs.lm_sensors ];
  closure = pkgs.closureInfo { rootPaths = diagnostics; };
in pkgs.runCommand "as7760-ram-probe" {
  nativeBuildInputs = [ pkgs.cpio pkgs.gzip pkgs.ubootTools pkgs.dtc pkgs.python3 ];
} ''
  mkdir -p $out root/bin root/dev root/proc root/sys root/run root/tmp root/etc
  cp ${targetPkgs.pkgsStatic.busybox}/bin/busybox root/bin/
  for app in sh mount cat echo ls find uname dmesg insmod rmmod lsmod ip \
    udhcpc sleep setsid cttyhack reboot poweroff hexdump readlink mkdir \
    grep sed awk head tail wc sync dd free ps; do
    ln -s busybox root/bin/$app
  done
  cp ${fanDriver}/lib/modules/${kernel.modDirVersion}/extra/as7760_fan.ko root/
  # Reuse the target tools and their exact loader/library closure in RAM.
  while IFS= read -r path; do
    cp -a --parents "$path" root/
  done < ${closure}/store-paths
  ln -s ${targetPkgs.ethtool}/bin/ethtool root/bin/ethtool
  ln -s ${targetPkgs.pciutils}/bin/lspci root/bin/lspci
  ln -s ${targetPkgs.usbutils}/bin/lsusb root/bin/lsusb
  ln -s ${targetPkgs.lm_sensors}/bin/sensors root/bin/sensors
  cp ${./ram-probe-init.sh} root/init
  cp ${./ram-dhcp.sh} root/etc/udhcpc.script
  chmod 0755 root/init root/etc/udhcpc.script
  (cd root; find . -print0 | sort -z | cpio --null -o -H newc --owner=0:0) | gzip -n -9 > $out/initrd.gz
  cp ${kernelImage}/uImage $out/uImage
  export SOURCE_DATE_EPOCH=1
  mkimage -A ppc -O linux -T ramdisk -C gzip -a 0 -e 0 \
    -n 'AS7760 RAM probe' -d $out/initrd.gz $out/uInitrd
  dtc -I dts -O dtb -o $out/as7760.dtb ${builtins.path { path = ../../packages/linux-qoriq-t2080/boards/as7760/as7760-vendor.dts; name = "as7760-vendor.dts"; }} 2> $out/dtc-diagnostics.txt
  python3 ${./validate-boot.py} $out ${kernelImage}/kernel.bin
  cat > $out/boot-commands.txt <<'EOF'
  # Supply the workstation's TFTP address in serverip; no saveenv.
  tftpboot 0x10000000 uImage
  tftpboot 0x20000000 uInitrd
  tftpboot 0x08000000 as7760.dtb
  setenv bootargs console=ttyS0,115200n8 rdinit=/init panic=0 pcie_aspm.policy=performance
  bootm 0x10000000 0x20000000 0x08000000
  EOF
''

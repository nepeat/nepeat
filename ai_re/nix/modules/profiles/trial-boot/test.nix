# trial-boot on QEMU: this module's boot.scr in the real alpine-v2 U-Boot (the
# qemu-virt build of the DMC-12 defconfig: same standard boot, hush, scsi, sysboot),
# AHCI disk laid out like the DMC-12's (MBR; sda1 /boot with boot.scr + extlinux.conf;
# sda2 root with the trial's extlinux.conf and its kernel files under /nix/store),
# Linux = the kernel-rust test kernel with a busybox /init that reports its role.
# Watchdog hooks are echo stubs here (the board's are proven on the board).
#   import ./test.nix { inherit pkgs; }     # pkgs: the build host's
{ pkgs }:
let
  lib = pkgs.lib;
  cross = pkgs.pkgsCross.aarch64-multiplatform;
  uboots = import ../../../packages/uboot { inherit pkgs; };
  uboot = (import ../../../packages/uboot/alpine-v2 { inherit pkgs uboots; }).qemu-virt;
  kernel = (import ../../../packages/kernel-rust { inherit pkgs; }).kernel;
  lba = 2047;
  loader = arm: {
    ubootArm = arm; ubootDisarm = [ "echo tb-test: disarm" ]; ubootDisarmOnDefault = true;
    confPartition = 2; confPath = "/var/lib/trial-boot/extlinux.conf";
  };
  scr = import ./boot-script.nix { inherit lib pkgs lba; loader = loader [ "echo tb-test: arm" ]; };
  scrArmFails = import ./boot-script.nix { inherit lib pkgs lba; loader = loader [ "echo tb-test: arm fails" "false" ]; };

  init = pkgs.writeText "init" ''
    #!/bin/sh
    mount -t proc proc /proc
    role=$(sed -n 's/.*role=\([a-z]*\).*/\1/p' /proc/cmdline)
    tb=$(sed -n 's/.*trialboot=\([0-9a-f]*\).*/\1/p' /proc/cmdline)
    echo "TB: role=$role trialboot=$tb"
    if [ "$role" = trial ]; then echo "TB: trial reboots"; reboot -f; else echo "TB: default powers off"; poweroff -f; fi
    sleep 60
  '';
  initrd = pkgs.runCommand "trial-boot-test-initrd" { nativeBuildInputs = [ pkgs.cpio pkgs.gzip ]; } ''
    mkdir -p root/bin root/proc
    cp ${cross.pkgsStatic.busybox}/bin/busybox root/bin/busybox
    for a in sh mount sed echo reboot poweroff sleep; do ln -s busybox root/bin/$a; done
    cp ${init} root/init; chmod +x root/init
    (cd root && find . | cpio -o -H newc --quiet | gzip -9) > $out
  '';
  # the machine's own DT for Linux (U-Boot's control DT is a fixture without what Linux needs)
  virtDtb = pkgs.runCommand "qemu-virt.dtb" { nativeBuildInputs = [ pkgs.qemu ]; } ''
    qemu-system-aarch64 -M virt,highmem=off,virtualization=on,gic-version=3,dumpdtb=$out -cpu cortex-a57 -m 1G -nographic
  '';
  store = "/nix/store/0123456789abcdfghijklmnpqrsvwxyz-trial";
  # one disk per scenario: with/without boot.scr, trial conf, trial command line
  mkDisk = name: { script ? scr, conf ? true, trialAppend ? "" }: pkgs.runCommand "trial-boot-disk-${name}" {
    nativeBuildInputs = [ pkgs.e2fsprogs pkgs.util-linux ];
  } ''
    mkdir -p boot/extlinux root${store} root/var/lib/trial-boot
    cp ${kernel}/Image boot/Image
    cp ${initrd} boot/initrd
    cp ${virtDtb} boot/test.dtb
    ${lib.optionalString (script != null) "cp ${script}/boot.scr boot/boot.scr"}
    cat > boot/extlinux/extlinux.conf <<'EOF'
    DEFAULT nixos-default
    TIMEOUT 1
    LABEL nixos-default
      LINUX ../Image
      INITRD ../initrd
      FDT ../test.dtb
      APPEND console=ttyAMA0 role=default
    EOF
    cp boot/Image boot/initrd boot/test.dtb root${store}/
    ${lib.optionalString conf ''
      cat > root/var/lib/trial-boot/extlinux.conf <<'EOF'
      # trial-boot abc123 (as trial-boot stage writes it)
      DEFAULT trial
      LABEL trial
        MENU LABEL trial abc123
        LINUX ${store}/Image
        INITRD ${store}/initrd
        FDT ${store}/test.dtb
        APPEND console=ttyAMA0 role=trial trialboot=abc123 panic=3 ${trialAppend}
      EOF
    ''}
    truncate -s 48M p1.img; mkfs.ext4 -q -L DMC12BOOT -E root_owner=0:0 -d boot p1.img
    truncate -s 48M p2.img; mkfs.ext4 -q -L DMC12ROOT -E root_owner=0:0 -d root p2.img
    mkdir $out; truncate -s 100M $out/disk.img
    printf 'label: dos\nstart=2048, size=98304, type=83, bootable\nstart=100352, size=98304, type=83\n' | sfdisk -q $out/disk.img
    dd if=p1.img of=$out/disk.img bs=512 seek=2048 conv=notrunc status=none
    dd if=p2.img of=$out/disk.img bs=512 seek=100352 conv=notrunc status=none
  '';
  disks = {
    base = mkDisk "base" { };
    panic = mkDisk "panic" { trialAppend = "rdinit=/bogus"; };
    noconf = mkDisk "noconf" { conf = false; };
    armfails = mkDisk "armfails" { script = scrArmFails; };
    noscript = mkDisk "noscript" { script = null; };
  };
in
pkgs.runCommand "trial-boot-qemu-test" {
  nativeBuildInputs = [ pkgs.qemu pkgs.python3 ];
  passthru = { inherit scr disks; };
} ''
  mkdir $out
  cp ${scr}/boot.cmd $out/
  python3 ${./test.py} ${pkgs.qemu}/bin/qemu-system-aarch64 ${uboot}/u-boot.bin ${toString lba} \
    base=${disks.base}/disk.img panic=${disks.panic}/disk.img noconf=${disks.noconf}/disk.img \
    armfails=${disks.armfails}/disk.img noscript=${disks.noscript}/disk.img $out 2>&1 | tee $out/log.txt
  grep -q '^PASS: trial-boot' $out/log.txt
''

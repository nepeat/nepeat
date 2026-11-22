{ config, lib, pkgs, modulesPath, ... }:
# Normal NixOS install on the SD card, built with nixpkgs' own sd-image module:
#   p1 FAT32 "NIXOS_BOOT" (256 MiB): uImage, uInitrd, DTB, boot.scr (+ prev/)
#   p2 ext4  "NIXOS_SD"  : /, /nix (grown to fill the card on first boot)
# Upstream U-Boot in NOR boots it with standard boot (nos_bootcmd = "bootflow
# scan -lb": extlinux.conf first, boot.scr second). boot.scr also serves the
# factory 2014 U-Boot ("fatload mmc 0:1 0x07000000 boot.scr; source 0x07000000").
# Every generation's toplevel carries its own boot files (as7760-boot/); the
# bootloader hook copies the active one to p1, keeping the previous in prev/.
# Deploy with: nixos-rebuild switch --target-host root@as7760 (devices/edge-core-7760-32x/deploy.sh)
#
# SD wear: p1 is noauto and only mounted by the hook; root is noatime; the
# journal is volatile (RAM, 1 GiB cap); /tmp is tmpfs; no swap.
let
  buildPkgs = pkgs.buildPackages;
  kernel = config.boot.kernelPackages.kernel;
  kernelImage = import ../../../packages/linux-qoriq-t2080/uimage.nix { pkgs = buildPkgs; targetPkgs = pkgs; inherit kernel; };
  uInitrd = buildPkgs.runCommand "as7760-uInitrd" { nativeBuildInputs = [ buildPkgs.ubootTools ]; } ''
    SOURCE_DATE_EPOCH=1 mkimage -A ppc -O linux -T ramdisk -C gzip -a 0 -e 0 \
      -n 'AS7760 NixOS initrd' -d ${config.system.build.initialRamdisk}/initrd $out
  '';
  dtb = buildPkgs.runCommand "as7760.dtb" { nativeBuildInputs = [ buildPkgs.dtc ]; } ''
    dtc -I dts -O dtb -o $out ${config.hardware.as7760.deviceTreeSource} 2>/dev/null
  '';
  fw = "/boot/firmware";
  # NixOS's own extlinux builder (generic-extlinux-compatible): extlinux.conf
  # with every generation for upstream U-Boot's standard boot (bootflow scan).
  extlinuxBuilderNix = modulesPath + "/system/boot/loader/generic-extlinux-compatible/extlinux-conf-builder.nix";
  extlinuxBuilder = import extlinuxBuilderNix { inherit lib pkgs; };
  extlinuxPopulate = import extlinuxBuilderNix { inherit lib; pkgs = pkgs.buildPackages; };
  extlinuxArgs = "-g 4 -t 3 -n as7760.dtb";
  dtbDir = buildPkgs.runCommand "as7760-dtbs" { } "mkdir $out; cp ${dtb} $out/as7760.dtb";
  installHook = pkgs.writeShellScript "as7760-install-boot" ''
    set -eu
    export PATH=${lib.makeBinPath [ pkgs.coreutils pkgs.util-linux pkgs.diffutils ]}
    sys=$1
    src=$sys/as7760-boot
    [ -s "$src/boot.scr" ] || { echo "as7760: $src has no boot files" >&2; exit 1; }
    mounted=
    mkdir -p ${fw}   # noauto mount: the mountpoint may not exist yet
    if ! mountpoint -q ${fw}; then mount ${fw}; mounted=1; fi
    mount -o remount,rw ${fw}
    cleanup() { sync; if [ -n "$mounted" ]; then umount ${fw}; else mount -o remount,ro ${fw}; fi; }
    trap cleanup EXIT
    put() { cmp -s "$1" "$2" 2>/dev/null || { cp "$1" "$2.new"; mv "$2.new" "$2"; }; }
    old=$(cat ${fw}/system 2>/dev/null || true)
    if [ -n "$old" ] && [ "$old" != "$sys" ] && [ -d "$old/as7760-boot" ]; then
      mkdir -p ${fw}/prev
      for f in uImage uInitrd as7760.dtb; do put "$old/as7760-boot/$f" ${fw}/prev/$f; done
      put "$old/as7760-boot/boot-prev.scr" ${fw}/prev/boot.scr
      echo "$old" > ${fw}/prev/system
    fi
    # boot.scr last: until it is replaced the old script still boots.
    for f in uImage uInitrd as7760.dtb boot.scr; do put "$src/$f" ${fw}/$f; done
    echo "$sys" > ${fw}/system
    # Standard boot for upstream U-Boot: /extlinux/extlinux.conf + /nixos/*.
    ${extlinuxBuilder} ${extlinuxArgs} -d ${fw} -c "$sys"
    echo "as7760: boot files for $sys installed on ${fw} (boot.scr + extlinux)"
  '';
in {
  imports = [ (modulesPath + "/installer/sd-card/sd-image.nix") ];

  # sd-image pulls in all-hardware; this kernel is a minimal built-in config.
  hardware.enableAllHardware = lib.mkForce false;

  sdImage = {
    firmwarePartitionName = "NIXOS_BOOT";
    firmwareSize = 256;
    compressImage = false;
    populateFirmwareCommands = ''
      for f in uImage uInitrd as7760.dtb boot.scr; do
        cp ${config.system.build.toplevel}/as7760-boot/$f firmware/$f
      done
      echo ${config.system.build.toplevel} > firmware/system
      ${extlinuxPopulate} ${extlinuxArgs} -d ./firmware -c ${config.system.build.toplevel}
    '';
    # U-Boot standard boot scans bootable partitions: make it the FAT one.
    postBuildCommands = "sfdisk --activate $img 1";
    populateRootCommands = "";
  };
  fileSystems."/".options = [ "noatime" "commit=60" ];
  fileSystems."/boot/firmware".options = [ "noatime" "fmask=0022" "dmask=0022" ];

  # Per-generation boot files inside the toplevel. boot-prev.scr is the same
  # script reading from prev/ (what the hook demotes the old generation to).
  system.systemBuilderCommands = ''
    mkdir $out/as7760-boot
    ln -s ${kernelImage}/uImage $out/as7760-boot/uImage
    ln -s ${uInitrd} $out/as7760-boot/uInitrd
    ln -s ${dtb} $out/as7760-boot/as7760.dtb
    for v in "" prev/; do
      {
        echo "fatload mmc 0:1 0x10000000 ${"$"}{v}uImage"
        echo "fatload mmc 0:1 0x20000000 ${"$"}{v}uInitrd"
        echo "fatload mmc 0:1 0x08000000 ${"$"}{v}as7760.dtb"
        echo "setenv bootargs ${toString config.boot.kernelParams} init=$out/init"
        echo "bootm 0x10000000 0x20000000 0x08000000"
      } > boot.cmd
      name=boot.scr; [ -n "$v" ] && name=boot-prev.scr
      SOURCE_DATE_EPOCH=1 ${buildPkgs.ubootTools}/bin/mkimage -A ppc -O linux -T script -C none \
        -n "AS7760 NixOS ${"$"}{v:-current}" -d boot.cmd $out/as7760-boot/$name >/dev/null
    done
    test "$(sed -n 4p boot.cmd | wc -c)" -lt 250 || { echo "bootargs too long for U-Boot"; exit 1; }
  '';
  boot.loader.external.installHook = lib.mkForce installHook;
  hardware.deviceTree = { enable = true; name = "as7760.dtb"; package = lib.mkForce dtbDir; };

  # Target-side pieces nixos-rebuild --target-host needs: a nix store/daemon
  # (to receive closures, set the profile) and switch-to-configuration.
  nix.enable = lib.mkForce true;
  nix.channel.enable = false;
  nix.settings = { trusted-users = [ "root" ]; auto-optimise-store = false; };
  nix.gc = { automatic = true; dates = "weekly"; options = "--delete-older-than 21d"; };
  system.switch.enable = lib.mkForce true;
  system.tools = {
    nixos-install.enable = false;
    nixos-generate-config.enable = false;
    nixos-enter.enable = false;
    nixos-build-vms.enable = false;
  };

  # The sd-image "sd-card" tag lengthens the init= path in bootargs; U-Boot
  # 2014's console buffer is 256 bytes.
  system.nixos.tags = lib.mkForce [ ];

  # Keep the card quiet between deploys.
  services.journald.settings.Journal = {
    Storage = lib.mkForce "volatile";
    RuntimeMaxUse = lib.mkForce "1G";
    RuntimeKeepFree = "2G";
  };
  services.fstrim.enable = false;
}
